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

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::completion::named_args::{
    extract_call_expression, find_enclosing_open_paren, position_to_char_offset,
    split_args_top_level,
};
use crate::php_type::PhpType;

use crate::symbol_map::SymbolMap;
use crate::types::*;
use crate::util::position_to_offset;

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
    // Active parameter = number of commas before the cursor.
    let active = cs
        .comma_offsets
        .iter()
        .filter(|&&comma| comma < cursor_byte_offset)
        .count() as u32;
    Some(CallSiteContext {
        call_expression: cs.call_expression.clone(),
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

    // Extract the call expression before `(`.
    let call_expr = extract_call_expression(&chars, open_paren)?;
    if call_expr.is_empty() {
        return None;
    }

    // Count the active parameter by splitting the text between `(` and the
    // cursor into top-level comma-separated segments.
    let args_text: String = chars[open_paren + 1..cursor].iter().collect();
    let segments = split_args_top_level(&args_text);
    // `split_args_top_level` returns one segment per completed comma-separated
    // argument (it omits a trailing empty segment).  The number of commas
    // equals the number of segments (each segment ended with a comma, except
    // possibly the last one which is the argument currently being typed).
    //
    // If the text ends with a comma (i.e. the cursor is right after `,`),
    // the split will have consumed it and the cursor is on the *next*
    // parameter.  Otherwise, the cursor is still on the segment after the
    // last comma.
    let trimmed = args_text.trim_end();
    let active = if trimmed.is_empty() {
        0
    } else if trimmed.ends_with(',') {
        segments.len() as u32
    } else {
        // The user is in the middle of typing an argument.  The number of
        // completed args equals segments.len() - 1 (last segment is the
        // current one) + 1 for the current, but we want a 0-based index
        // so it's segments.len() - 1.  However split_args_top_level may
        // or may not include the trailing segment.  Counting commas
        // directly is more reliable.
        count_top_level_commas(&chars, open_paren + 1, cursor)
    };

    Some(CallSiteContext {
        call_expression: call_expr,
        active_parameter: active,
    })
}

/// Count commas at nesting depth 0 between `start` (inclusive) and `end`
/// (exclusive) in a char slice, skipping nested parens/brackets and
/// string literals.
fn count_top_level_commas(chars: &[char], start: usize, end: usize) -> u32 {
    let mut count = 0u32;
    let mut depth = 0i32;
    let mut i = start;

    while i < end {
        match chars[i] {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            ',' if depth == 0 => count += 1,
            '\'' | '"' => {
                let q = chars[i];
                i += 1;
                while i < end {
                    if chars[i] == q {
                        let mut backslashes = 0u32;
                        let mut k = i;
                        while k > start && chars[k - 1] == '\\' {
                            backslashes += 1;
                            k -= 1;
                        }
                        if backslashes.is_multiple_of(2) {
                            break;
                        }
                    }
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }

    count
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
    // The parameters start right after the `(`.
    let mut offset = 1; // skip the leading `(`

    for (idx, (pl, param)) in param_labels.iter().zip(params.iter()).enumerate() {
        let start = offset as u32;
        let end = (offset + pl.len()) as u32;
        param_infos.push(ParameterInformation {
            label: ParameterLabel::LabelOffsets([start, end]),
            documentation: build_param_documentation(param),
        });
        // Move past this parameter label and the separator `, `.
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
            parameters: t.parameters,
            return_type: t.return_type,
        }
    }
}

// ─── First-class callable target extraction (AST-based) ────────────────────

use mago_span::HasSpan;
use mago_syntax::cst::*;

/// Walk statements looking for the last assignment to `var_name` (with
/// `$` prefix) before `cursor_offset` where the RHS is a
/// `PartialApplication` (first-class callable).  Returns the callable
/// target text extracted from `content`.
fn find_fcc_target_in_stmts<'a, I>(
    stmts: I,
    var_name: &str,
    cursor_offset: u32,
    content: &str,
) -> Option<String>
where
    I: Iterator<Item = &'a Statement<'a>>,
{
    let mut best: Option<String> = None;
    for stmt in stmts {
        collect_fcc_targets(stmt, var_name, cursor_offset, content, &mut best);
    }
    best
}

/// Recursively collect first-class callable assignment targets from a
/// statement, updating `best` with the latest match found.
fn collect_fcc_targets(
    stmt: &Statement<'_>,
    var_name: &str,
    cursor_offset: u32,
    content: &str,
    best: &mut Option<String>,
) {
    /// Helper macro: walk an iterable of statements.
    macro_rules! walk {
        ($iter:expr) => {
            for s in $iter {
                collect_fcc_targets(s, var_name, cursor_offset, content, best);
            }
        };
    }

    match stmt {
        Statement::Expression(expr_stmt) => {
            if let Some(found) =
                find_fcc_target_in_expr(expr_stmt.expression, var_name, cursor_offset, content)
            {
                *best = Some(found);
            }
        }
        Statement::Block(block) => walk!(block.statements.iter()),
        Statement::Namespace(ns) => walk!(ns.statements().iter()),
        Statement::Class(cls) => {
            use mago_syntax::cst::class_like::member::ClassLikeMember;
            use mago_syntax::cst::class_like::method::MethodBody;
            for member in cls.members.iter() {
                if let ClassLikeMember::Method(method) = member
                    && let MethodBody::Concrete(block) = &method.body
                {
                    let span = method.span();
                    if cursor_offset >= span.start.offset && cursor_offset <= span.end.offset {
                        walk!(block.statements.iter());
                        return;
                    }
                }
            }
        }
        Statement::Trait(t) => {
            use mago_syntax::cst::class_like::member::ClassLikeMember;
            use mago_syntax::cst::class_like::method::MethodBody;
            for member in t.members.iter() {
                if let ClassLikeMember::Method(method) = member
                    && let MethodBody::Concrete(block) = &method.body
                {
                    let span = method.span();
                    if cursor_offset >= span.start.offset && cursor_offset <= span.end.offset {
                        walk!(block.statements.iter());
                        return;
                    }
                }
            }
        }
        Statement::Function(func) => {
            let span = func.body.span();
            if cursor_offset >= span.start.offset && cursor_offset <= span.end.offset {
                walk!(func.body.statements.iter());
            }
        }
        Statement::If(if_stmt) => match &if_stmt.body {
            IfBody::Statement(body) => {
                collect_fcc_targets(body.statement, var_name, cursor_offset, content, best);
                for ei in body.else_if_clauses.iter() {
                    collect_fcc_targets(ei.statement, var_name, cursor_offset, content, best);
                }
                if let Some(ref else_clause) = body.else_clause {
                    collect_fcc_targets(
                        else_clause.statement,
                        var_name,
                        cursor_offset,
                        content,
                        best,
                    );
                }
            }
            IfBody::ColonDelimited(body) => {
                walk!(body.statements.iter());
                for ei in body.else_if_clauses.iter() {
                    walk!(ei.statements.iter());
                }
                if let Some(ref else_clause) = body.else_clause {
                    walk!(else_clause.statements.iter());
                }
            }
        },
        Statement::Foreach(foreach) => match &foreach.body {
            ForeachBody::Statement(inner) => {
                collect_fcc_targets(inner, var_name, cursor_offset, content, best);
            }
            ForeachBody::ColonDelimited(body) => walk!(body.statements.iter()),
        },
        Statement::For(for_stmt) => match &for_stmt.body {
            ForBody::Statement(inner) => {
                collect_fcc_targets(inner, var_name, cursor_offset, content, best);
            }
            ForBody::ColonDelimited(body) => walk!(body.statements.iter()),
        },
        Statement::While(while_stmt) => match &while_stmt.body {
            WhileBody::Statement(inner) => {
                collect_fcc_targets(inner, var_name, cursor_offset, content, best);
            }
            WhileBody::ColonDelimited(body) => walk!(body.statements.iter()),
        },
        Statement::DoWhile(do_while) => {
            collect_fcc_targets(do_while.statement, var_name, cursor_offset, content, best);
        }
        Statement::Try(try_stmt) => {
            walk!(try_stmt.block.statements.iter());
            for catch in try_stmt.catch_clauses.iter() {
                walk!(catch.block.statements.iter());
            }
            if let Some(ref finally) = try_stmt.finally_clause {
                walk!(finally.block.statements.iter());
            }
        }
        _ => {}
    }
}

/// Check if an expression is an assignment to `var_name` with a
/// `PartialApplication` RHS, and extract the callable target text.
fn find_fcc_target_in_expr(
    expr: &Expression<'_>,
    var_name: &str,
    cursor_offset: u32,
    content: &str,
) -> Option<String> {
    if let Expression::Assignment(assignment) = expr {
        // Only consider assignments that end before the cursor.
        let assign_end = assignment.span().end.offset;
        if assign_end > cursor_offset {
            return None;
        }

        // Check the LHS is the variable we're looking for.
        if let Expression::Variable(Variable::Direct(dv)) = assignment.lhs {
            if dv.name != var_name.as_bytes() {
                return None;
            }

            // Check if the RHS is a PartialApplication.
            if let Expression::PartialApplication(pa) = assignment.rhs {
                return extract_callable_text_from_partial(pa, content);
            }
        }
    }
    None
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
        let ctx = self.file_context(uri);

        // ── Early bail-out: cursor inside a closure/arrow-fn body ───
        // When the cursor is inside a closure or arrow function body
        // that is itself an argument to a call, suppress signature help
        // for that outer call.  The user is writing code *inside* the
        // closure, not filling in arguments to the outer call.
        //
        // This check runs once, before both detection paths, so it
        // covers the AST-based path and the text-based fallback alike.
        let symbol_map = self.symbol_maps.read().get(uri).cloned();
        if let Some(ref sm) = symbol_map {
            let cursor_offset = position_to_offset(content, position);
            if let Some(call) = sm.find_enclosing_call_site(cursor_offset)
                && sm.is_inside_nested_scope_of_call(cursor_offset, call)
            {
                return None;
            }
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
                let patched_classes: Vec<Arc<crate::types::ClassInfo>> =
                    self.parse_php(&patched).into_iter().map(Arc::new).collect();
                if !patched_classes.is_empty() {
                    let patched_ctx = FileContext {
                        classes: patched_classes,
                        use_map: ctx.use_map.clone(),
                        namespace: ctx.namespace.clone(),
                        resolved_names: ctx.resolved_names.clone(),
                    };
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

    /// Resolve the call expression to a `SignatureHelp` using the given
    /// file context and content.
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
            active_parameter: Some(clamp_active_param(
                site.active_parameter,
                &resolved.parameters,
            )),
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

    /// Find the callable target for a first-class callable assignment
    /// like `$fn = someTarget(...)` and return the callable target string
    /// (e.g. `"makePen"`, `"$obj->method"`, `"ClassName::method"`).
    ///
    /// This enables signature help for first-class callable invocations:
    /// `$fn = makePen(...); $fn()` shows `makePen`'s parameters.
    ///
    /// Uses the AST to find assignments to `var_name` before
    /// `cursor_offset` where the RHS is a `PartialApplication` node,
    /// then extracts the callable portion from the source text.
    pub(crate) fn extract_callable_target_from_variable(
        var_name: &str,
        content: &str,
        cursor_offset: u32,
    ) -> Option<String> {
        crate::parser::with_parsed_program(
            content,
            "extract_callable_target",
            |program, content| {
                find_fcc_target_in_stmts(
                    program.statements.iter(),
                    var_name,
                    cursor_offset,
                    content,
                )
            },
        )
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
        let byte_offset = crate::util::position_to_byte_offset(content, position);
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

// ─── Definition-site suppression ────────────────────────────────────────────

/// Check whether the open parenthesis at `paren_pos` belongs to a
/// function or method *definition* rather than a call expression.
///
/// Walks backward from `(` through the function name (if any), then
/// through whitespace, looking for the `function` or `fn` keyword.
/// Returns `true` for `function foo(`, `function (`, `fn(`, and
/// `public function bar(`.
fn is_function_definition_paren(chars: &[char], paren_pos: usize) -> bool {
    let mut i = paren_pos;

    // Skip whitespace before `(`
    while i > 0 && chars[i - 1].is_ascii_whitespace() {
        i -= 1;
    }

    // Walk backward through the identifier (function name, which may be
    // empty for anonymous functions / closures).
    let name_end = i;
    while i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_') {
        i -= 1;
    }

    let name: String = chars[i..name_end].iter().collect();

    // Anonymous `function(` or arrow `fn(` — the name *is* the keyword.
    if name == "function" || name == "fn" {
        return true;
    }

    // Named function / method: skip whitespace before the name and check
    // for the `function` or `fn` keyword.
    let mut j = i;
    while j > 0 && chars[j - 1].is_ascii_whitespace() {
        j -= 1;
    }

    if ends_with_keyword(chars, j, "function") || ends_with_keyword(chars, j, "fn") {
        return true;
    }

    false
}

/// Check whether the text ending at `pos` (exclusive) ends with `keyword`
/// on a word boundary.
fn ends_with_keyword(chars: &[char], pos: usize, keyword: &str) -> bool {
    let kw_len = keyword.len();
    if pos < kw_len {
        return false;
    }
    let start = pos - kw_len;
    let candidate: String = chars[start..pos].iter().collect();
    if candidate != keyword {
        return false;
    }
    // Ensure word boundary before the keyword.
    if start > 0 && (chars[start - 1].is_alphanumeric() || chars[start - 1] == '_') {
        return false;
    }
    true
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "signature_help_tests.rs"]
mod tests;
