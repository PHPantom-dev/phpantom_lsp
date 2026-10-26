//! Signature help (`textDocument/signatureHelp`).
//!
//! When the cursor is inside the parentheses of a function or method call,
//! this module resolves the callable and returns its signature (parameter
//! names, types, and return type) along with the index of the parameter
//! currently being typed.
//!
//! The primary detection path uses precomputed [`CallSite`] data from the
//! [`SymbolMap`] (AST-based, handles chains and nesting correctly).  When
//! the symbol map has no matching call site (e.g. the parser couldn't
//! recover an unclosed paren), we fall back to text-based backward
//! scanning so that signature help still works on incomplete code.

use std::sync::Arc;

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::completion::named_args::{
    extract_call_expression, find_enclosing_open_paren, is_function_definition_paren,
    position_to_char_offset,
};
use crate::php_type::PhpType;

use crate::symbol_map::SymbolMap;
use crate::text_position::position_to_offset;
use crate::types::*;

/// Information about a signature help call site, extracted from the source
/// text around the cursor.
struct CallSiteContext {
    /// The call expression in a format suitable for resolution (same
    /// format as [`NamedArgContext::call_expression`]).
    call_expression: String,
    /// Zero-based index of the parameter the cursor is currently on,
    /// determined by counting top-level commas before the cursor.
    active_parameter: u32,
}

// ─── AST-based detection ────────────────────────────────────────────────────

/// Detect the call site using precomputed [`CallSite`] data from the
/// symbol map.
///
/// Converts the LSP `Position` to a byte offset, finds the innermost
/// `CallSite` whose argument list contains the cursor, and computes the
/// active parameter index from the precomputed comma offsets.
fn detect_call_site_from_map(
    symbol_map: &SymbolMap,
    content: &str,
    position: Position,
) -> Option<CallSiteContext> {
    let cursor_byte_offset = position_to_offset(content, position);
    let cs = symbol_map.find_enclosing_call_site(cursor_byte_offset)?;
    let active = cs
        .comma_offsets
        .iter()
        .filter(|&&comma| comma < cursor_byte_offset)
        .count() as u32;
    Some(CallSiteContext {
        call_expression: cs.call_expression.to_string(),
        active_parameter: active,
    })
}

// ─── Text-based detection (fallback) ────────────────────────────────────────

/// Detect whether the cursor is inside a function/method call using
/// text-based backward scanning.
///
/// This is the **fallback** path used when the AST-based detection
/// (via `detect_call_site_from_map`) has no hit — typically because the
/// parser couldn't recover the call node from incomplete code (e.g. an
/// unclosed `(`).
///
/// Returns `None` if the cursor is not inside call parentheses.
fn detect_call_site_text_fallback(content: &str, position: Position) -> Option<CallSiteContext> {
    let chars: Vec<char> = content.chars().collect();
    let cursor = position_to_char_offset(&chars, position)?;

    // Find the enclosing open paren.  We search backward from the cursor
    // position itself (not from a word-start like named-arg detection does)
    // because signature help should fire even when the cursor is right
    // after a comma or the open paren with no identifier typed yet.
    let open_paren = find_enclosing_open_paren(&chars, cursor)?;

    // Suppress signature help when the cursor is inside the parameter
    // list of a function/method *definition* rather than a call.
    // Method definitions are already suppressed because
    // `extract_call_expression` returns a bare method name that
    // doesn't resolve as a callable.  Standalone function definitions
    // (`function foo(`) *do* resolve when a global function with the
    // same name exists, so we need an explicit check.
    if is_function_definition_paren(&chars, open_paren) {
        return None;
    }

    let call_expr = extract_call_expression(&chars, open_paren)?;
    if call_expr.is_empty() {
        return None;
    }

    // The active parameter is the number of top-level commas between the
    // `(` and the cursor.
    let args_text: String = chars[open_paren + 1..cursor].iter().collect();
    let active = crate::text_scan::top_level_commas(&args_text).count() as u32;

    Some(CallSiteContext {
        call_expression: call_expr,
        active_parameter: active,
    })
}

// ─── Signature building ─────────────────────────────────────────────────────

/// Format a single parameter for the signature label using the **native**
/// PHP type hint (not the docblock-enriched effective type).
///
/// Optional (non-variadic) parameters with a known default value include
/// ` = <value>` so the user can see the default at a glance.
fn format_param_label(param: &ParameterInfo) -> String {
    let mut parts = Vec::new();
    if let Some(ref th) = param.native_type_hint {
        parts.push(th.to_string());
    }
    if param.is_variadic {
        parts.push(format!("...{}", param.name));
    } else if param.is_reference {
        parts.push(format!("&{}", param.name));
    } else {
        parts.push(param.name.to_string());
    }
    let base = parts.join(" ");
    if !param.is_required
        && !param.is_variadic
        && let Some(ref dv) = param.default_value
    {
        return format!("{} = {}", base, dv);
    }
    base
}

/// Build per-parameter documentation markdown.
///
/// When the effective type (`type_hint`) differs from the native PHP type
/// (`native_type_hint`), the effective type is shown (shortened to base
/// names) in an inline code span so the user sees the richer docblock
/// type.  If a `@param` description is also present it is appended after
/// the type.
///
/// Returns `None` when there is nothing to show (no description and the
/// types are identical or absent).
fn build_param_documentation(param: &ParameterInfo) -> Option<Documentation> {
    let native = param.native_type_hint.as_ref();
    let desc = param.description.as_deref();

    let show_effective = match (&param.type_hint, native) {
        (Some(eff), Some(nat)) => !eff.equivalent(nat),
        (Some(_), None) => true,
        _ => false,
    };

    let shortened = param.type_hint.as_ref().map(crate::hover::shorten_php_type);
    let value = match (show_effective, desc) {
        (true, Some(d)) => format!("`{}` {}", shortened.as_deref().unwrap_or(""), d),
        (true, None) => format!("`{}`", shortened.as_deref().unwrap_or("")),
        (false, Some(d)) => d.to_string(),
        (false, None) => return None,
    };

    Some(Documentation::MarkupContent(MarkupContent {
        kind: MarkupKind::Markdown,
        value,
    }))
}

/// Build a `SignatureInformation` from a callable's metadata.
///
/// The label uses **native** PHP types for parameters and a shortened
/// (base-name) effective return type.  Per-parameter documentation shows
/// the `@param` description, optionally prefixed with the effective type
/// when it differs from the native type.
fn build_signature(
    params: &[ParameterInfo],
    return_type: Option<&PhpType>,
) -> SignatureInformation {
    // Build the label: `(param1, param2, ...): ReturnType`
    // The callable name is omitted — the user already knows what they
    // are calling and the editor shows it in the surrounding code.
    let param_labels: Vec<String> = params.iter().map(format_param_label).collect();
    let params_str = param_labels.join(", ");
    let ret = format!(
        ": {}",
        return_type.map_or_else(
            || PhpType::mixed().to_string(),
            crate::hover::shorten_php_type
        )
    );
    let label = format!("({}){}", params_str, ret);

    // Build ParameterInformation using label offsets.  The offsets are
    // byte offsets into the label string (UTF-16 code unit offsets are
    // also accepted, but since PHP identifiers are ASCII the byte
    // offsets match).
    let mut param_infos = Vec::with_capacity(params.len());
    let mut offset = 1; // skip the leading `(`

    for (idx, (pl, param)) in param_labels.iter().zip(params.iter()).enumerate() {
        let start = offset as u32;
        let end = (offset + pl.len()) as u32;
        param_infos.push(ParameterInformation {
            label: ParameterInformationLabel::Tuple((start, end)),
            documentation: build_param_documentation(param),
        });
        offset += pl.len();
        if idx < param_labels.len() - 1 {
            offset += 2; // ", "
        }
    }

    SignatureInformation {
        label,
        documentation: None,
        parameters: Some(param_infos),
        active_parameter: None,
    }
}

// ─── Resolution ─────────────────────────────────────────────────────────────

/// Resolved callable information ready to be turned into a
/// `SignatureHelp` response.
///
/// This is a thin projection of [`ResolvedCallableTarget`] kept for
/// local use within this module.  Only the fields actually consumed by
/// `resolve_signature` are retained.
struct ResolvedCallable {
    /// The parameters of the callable.
    parameters: Vec<ParameterInfo>,
    /// Optional return type (effective / docblock-enriched).
    return_type: Option<PhpType>,
}

impl From<crate::types::ResolvedCallableTarget> for ResolvedCallable {
    fn from(t: crate::types::ResolvedCallableTarget) -> Self {
        Self {
            parameters: t.parameters.into_vec(),
            return_type: t.return_type,
        }
    }
}

// ─── First-class callable target extraction (AST-based) ────────────────────

use crate::atom::bytes_to_str;
use mago_span::HasSpan;
use mago_syntax::cst::*;

/// A partial application assignment (`$fn = target(...)`, `$fn =
/// target(?, 1)`) recorded by [`collect_fcc_assignments`], or a later
/// assignment of some other value to a variable that held one.
pub(crate) struct FccAssignment {
    /// The assigned variable, with its `$` prefix.
    var_name: Box<str>,
    /// Where the assignment expression ends.
    end: u32,
    /// The partial application assigned, or `None` when the variable is
    /// assigned some other value and no longer holds it.
    callable: Option<AssignedCallable>,
    /// The span of the innermost function or method the assignment sits
    /// in, or `None` at file scope.  It is only visible from inside it.
    scope: Option<(u32, u32)>,
}

/// The partial application a variable holds.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AssignedCallable {
    /// The callable target text (`makePen`, `$obj->method`, `Foo::bar`).
    pub(crate) target: String,
    /// The argument text a partial application with placeholders was
    /// written with (`1, ?`), or `None` for a first-class callable.
    pub(crate) bound: Option<String>,
}

/// Every first-class callable assignment in `statements`, in source
/// order.  File-scope assignments and those in function and method bodies
/// are recorded; those inside closures are not.
pub(crate) fn collect_fcc_assignments(
    statements: &[Statement<'_>],
    content: &str,
) -> Vec<FccAssignment> {
    let mut out = Vec::new();
    for stmt in statements {
        collect_fcc_targets(stmt, None, content, &mut out);
    }
    out
}

/// The partial application `var_name` (with `$` prefix) holds at
/// `cursor_offset`: the last assignment to it that ends before the cursor
/// and is visible from it, when that assigns one.
fn find_fcc_target(
    assignments: &[FccAssignment],
    var_name: &str,
    cursor_offset: u32,
) -> Option<AssignedCallable> {
    assignments
        .iter()
        .rev()
        .find(|a| {
            &*a.var_name == var_name
                && a.end <= cursor_offset
                && a.scope
                    .is_none_or(|(start, end)| cursor_offset >= start && cursor_offset <= end)
        })?
        .callable
        .clone()
}

/// Recursively collect first-class callable assignments from a statement
/// into `out`, tagging each with the function-like `scope` it sits in.
fn collect_fcc_targets(
    stmt: &Statement<'_>,
    scope: Option<(u32, u32)>,
    content: &str,
    out: &mut Vec<FccAssignment>,
) {
    macro_rules! walk {
        ($iter:expr, $scope:expr) => {
            for s in $iter {
                collect_fcc_targets(s, $scope, content, out);
            }
        };
        ($iter:expr) => {
            walk!($iter, scope)
        };
    }
    let span_of = |span: mago_span::Span| Some((span.start.offset, span.end.offset));

    match stmt {
        Statement::Expression(expr_stmt) => {
            if let Some(found) = fcc_assignment_in_expr(expr_stmt.expression, scope, content)
                // Another value only matters to a variable that held a
                // callable, so the rest are not recorded.
                && (found.callable.is_some()
                    || out.iter().any(|a| a.var_name == found.var_name))
            {
                out.push(found);
            }
        }
        _ if let Some(body) = crate::parser::class_like_body(stmt) => {
            crate::parser::for_each_concrete_method(body.members, |method, block| {
                walk!(block.statements.iter(), span_of(method.span()));
            });
        }
        Statement::Function(func) => {
            walk!(func.body.statements.iter(), span_of(func.body.span()));
        }
        _ => crate::parser::for_each_nested_statement_list(stmt, |nested| walk!(nested.iter())),
    }
}

/// The assignment to a variable `expr` makes, recording the partial
/// application it assigns (`$fn = target(...)`), if any.
fn fcc_assignment_in_expr(
    expr: &Expression<'_>,
    scope: Option<(u32, u32)>,
    content: &str,
) -> Option<FccAssignment> {
    let Expression::Assignment(assignment) = expr else {
        return None;
    };
    let Expression::Variable(Variable::Direct(dv)) = assignment.lhs else {
        return None;
    };
    let callable = match assignment.rhs {
        Expression::PartialApplication(pa) => Some(AssignedCallable {
            target: extract_callable_text_from_partial(pa, content)?,
            bound: (!pa.is_first_class_callable())
                .then(|| {
                    let list = pa.get_argument_list();
                    content.get(
                        list.left_parenthesis.end.offset as usize
                            ..list.right_parenthesis.start.offset as usize,
                    )
                })
                .flatten()
                .map(|text| text.trim().to_string()),
        }),
        _ => None,
    };
    Some(FccAssignment {
        var_name: bytes_to_str(dv.name).into(),
        end: assignment.span().end.offset,
        callable,
        scope,
    })
}

/// Extract the callable target text from a `PartialApplication` AST
/// node by reading the source text for the callee portion (everything
/// before the argument list).
fn extract_callable_text_from_partial(
    pa: &partial_application::PartialApplication<'_>,
    content: &str,
) -> Option<String> {
    let (callee_start, callee_end) = match pa {
        partial_application::PartialApplication::Function(f) => {
            let span = f.function.span();
            (span.start.offset as usize, span.end.offset as usize)
        }
        partial_application::PartialApplication::Method(m) => {
            let start = m.object.span().start.offset as usize;
            let end = m.method.span().end.offset as usize;
            (start, end)
        }
        partial_application::PartialApplication::StaticMethod(s) => {
            let start = s.class.span().start.offset as usize;
            let end = s.method.span().end.offset as usize;
            (start, end)
        }
    };

    let text = content.get(callee_start..callee_end)?.trim();
    if text.is_empty() {
        return None;
    }
    Some(text.to_string())
}

impl Backend {
    /// Handle a `textDocument/signatureHelp` request.
    ///
    /// Returns `Some(SignatureHelp)` when the cursor is inside a
    /// function or method call and the callable can be resolved, or
    /// `None` otherwise.
    ///
    /// Detection strategy:
    /// 1. **AST-based** — look up the precomputed `CallSite` in the
    ///    symbol map.  This handles chains, nesting, and strings correctly.
    /// 2. **Text fallback** — when the symbol map has no hit (e.g. the
    ///    parser couldn't recover the call node from incomplete code),
    ///    fall back to the text-based backward scanner.
    pub(crate) fn handle_signature_help(
        &self,
        uri: &str,
        content: &str,
        position: Position,
    ) -> Option<SignatureHelp> {
        let cursor_offset = position_to_offset(content, position);
        let ctx = self.file_context_at(uri, cursor_offset);

        // ── Early bail-out: cursor inside a closure/arrow-fn body ───
        // When the cursor is inside a closure or arrow function body
        // that is itself an argument to a call, suppress signature help
        // for that outer call.  The user is writing code *inside* the
        // closure, not filling in arguments to the outer call.
        //
        // This check runs once, before both detection paths, so it
        // covers the AST-based path and the text-based fallback alike.
        let symbol_map = self.symbol_map_for(uri);
        if let Some(ref sm) = symbol_map
            && let Some(call) = sm.find_enclosing_call_site(cursor_offset)
            && sm.is_inside_nested_scope_of_call(cursor_offset, call)
        {
            return None;
        }

        // ── Primary path: AST-based detection via symbol map ────────
        if let Some(ref sm) = symbol_map
            && let Some(site) = detect_call_site_from_map(sm, content, position)
            && let Some(result) = self.resolve_signature(&site, content, position, &ctx)
        {
            return Some(result);
        }

        // ── Fallback: text-based detection ──────────────────────────
        // The parser may not have produced a call node (e.g. unclosed
        // paren while typing).  The text scanner handles this because
        // it only needs an unmatched `(`.
        if let Some(site) = detect_call_site_text_fallback(content, position) {
            // Try with current AST first.
            if let Some(result) = self.resolve_signature(&site, content, position, &ctx) {
                return Some(result);
            }

            // Patch content (insert `);` at cursor) and retry with
            // a re-parsed AST so resolution can find class context.
            let patched = Self::patch_content_for_signature(content, position);
            if patched != content {
                let patched_classes: Vec<Arc<crate::types::ClassInfo>> = self
                    .parse_php_resolved(&patched)
                    .into_iter()
                    .map(Arc::new)
                    .collect();
                if !patched_classes.is_empty() {
                    let patched_ctx = ctx.with_classes(patched_classes);
                    if let Some(result) =
                        self.resolve_signature(&site, &patched, position, &patched_ctx)
                    {
                        return Some(result);
                    }
                }
            }
        }

        None
    }

    fn resolve_signature(
        &self,
        site: &CallSiteContext,
        content: &str,
        position: Position,
        ctx: &FileContext,
    ) -> Option<SignatureHelp> {
        let resolved = self.resolve_callable(&site.call_expression, content, position, ctx)?;

        let sig = build_signature(&resolved.parameters, resolved.return_type.as_ref());
        Some(SignatureHelp {
            signatures: vec![sig],
            active_signature: Some(0),
            active_parameter: Some(ActiveParameter::Int(clamp_active_param(
                site.active_parameter,
                &resolved.parameters,
            ))),
        })
    }

    /// Resolve a call expression string to the callable's metadata.
    ///
    /// Delegates to the shared [`Backend::resolve_callable_target`] and
    /// converts the result into the local [`ResolvedCallable`] type.
    fn resolve_callable(
        &self,
        expr: &str,
        content: &str,
        position: Position,
        ctx: &FileContext,
    ) -> Option<ResolvedCallable> {
        self.resolve_callable_target(expr, content, position, ctx)
            .map(ResolvedCallable::from)
    }

    /// Find the partial application a variable holds, from an assignment
    /// like `$fn = someTarget(...)`: its callable target string (e.g.
    /// `"makePen"`, `"$obj->method"`, `"ClassName::method"`) and the
    /// arguments it binds.
    ///
    /// This enables signature help for first-class callable invocations:
    /// `$fn = makePen(...); $fn()` shows `makePen`'s parameters.
    ///
    /// Looks the assignment up among the file's partial application
    /// assignments, which the active parse cache collects once per pass.
    pub(crate) fn extract_callable_target_from_variable(
        var_name: &str,
        content: &str,
        cursor_offset: u32,
    ) -> Option<AssignedCallable> {
        crate::parser::with_fcc_assignments(content, |assignments| {
            find_fcc_target(assignments, var_name, cursor_offset)
        })
    }

    /// The call invoking `var_name` with `call_args` at `cursor_offset`
    /// makes, as source text, when the variable holds a partial application
    /// of a named function or method: `$fn = $r->make(1, ?); $fn($class)`
    /// calls `$r->make(1, $class)`.
    ///
    /// `None` for a partial application of another callable value, whose
    /// type already says what invoking it returns.
    pub(crate) fn invoked_partial_application_call(
        var_name: &str,
        content: &str,
        cursor_offset: u32,
        call_args: &str,
    ) -> Option<String> {
        let assigned =
            Self::extract_callable_target_from_variable(var_name, content, cursor_offset)?;
        if assigned.target.starts_with('$') && !assigned.target.contains("->") {
            return None;
        }
        let args = match assigned.bound.as_deref() {
            Some(bound) => crate::call_args::bind_partial_text_arguments(bound, call_args),
            None => call_args.to_string(),
        };
        Some(format!("{}({})", assigned.target, args))
    }

    /// Insert `);` at the cursor position so that an unclosed call
    /// expression becomes syntactically valid.
    ///
    /// This is the same patching strategy used by named-argument
    /// completion (see `handler::patch_content_at_cursor`).
    fn patch_content_for_signature(content: &str, position: Position) -> String {
        // Convert the LSP position (UTF-16 column) to an absolute byte
        // offset, preserving the original line terminators (including
        // CRLF), then splice `);` in at that point.
        let byte_offset = crate::text_position::position_to_byte_offset(content, position);
        let mut result = String::with_capacity(content.len() + 2);
        result.push_str(&content[..byte_offset]);
        result.push_str(");");
        result.push_str(&content[byte_offset..]);
        result
    }
}

/// Clamp the active parameter index so it doesn't exceed the parameter
/// count.  For variadic parameters, the index stays on the last parameter
/// even when the user types additional arguments.
fn clamp_active_param(active: u32, params: &[ParameterInfo]) -> u32 {
    if params.is_empty() {
        return 0;
    }
    let last = (params.len() - 1) as u32;
    active.min(last)
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "signature_help_tests.rs"]
mod tests;
