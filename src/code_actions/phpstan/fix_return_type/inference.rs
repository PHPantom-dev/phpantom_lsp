//! Return-type inference from function bodies.
//!
//! Walks the `return` statements of a function via the AST and
//! resolves each returned expression's type through the shared RHS
//! resolution pipeline (the same one hover, completion, and
//! diagnostics use).

use std::collections::HashMap;
use std::sync::Arc;

use mago_syntax::cst::expression::Expression;
use mago_syntax::cst::variable::Variable;
use tower_lsp::lsp_types::Position;

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::completion::resolver::{Loaders, VarResolutionCtx};
use crate::completion::variable::foreach_resolution::resolve_expression_type;
use crate::parser::with_parsed_program;
use crate::php_type::PhpType;
use crate::return_collection::collect_returns;
use crate::text_position::line_start_byte_offset;
use crate::types::{ClassInfo, FunctionLoader};

use super::edits::find_open_brace_from_declaration;

// ── Return type inference result ────────────────────────────────────────────

/// The result of inferring a return type from a function body.
///
/// Separates the native PHP type hint (for the `: type` declaration)
/// from the effective PHPStan type (for a `@return` docblock tag).
/// When the two are identical, no docblock is needed.
pub(crate) struct InferredReturnType {
    /// Valid native PHP type hint (e.g. `array`, `int`, `Foo`).
    pub(crate) native: PhpType,
    /// Full effective type including generics/shapes (e.g. `list<string>`).
    /// `None` when the native type already captures the full type.
    pub(crate) effective: Option<PhpType>,
}

// ── Backend methods ─────────────────────────────────────────────────────────

impl Backend {
    /// Infer the return type of the function at `func_line` by scanning
    /// all return statements in the body.
    ///
    /// Returns an [`InferredReturnType`] that separates the native PHP
    /// type hint from the richer effective type.  When they differ (e.g.
    /// `list<string>` vs `array`), the caller should add a `@return` tag.
    ///
    /// When `self_as_marker` is `true`, `return $this;` yields the self-like
    /// marker `$this` so the type engine can map it to the receiver class
    /// rather than the (possibly trait) class that declares the method.
    pub(crate) fn infer_return_type_for_function(
        &self,
        uri: &str,
        content: &str,
        func_line: usize,
        self_as_marker: bool,
    ) -> Option<InferredReturnType> {
        // Set up the resolution infrastructure from Backend state.
        let local_classes: Vec<Arc<ClassInfo>> = self
            .uri_classes_index
            .read()
            .get(uri)
            .cloned()
            .unwrap_or_default();
        let file_use_map: HashMap<String, String> = self.file_use_map(uri);
        let file_namespace: Option<String> = self.first_file_namespace(uri);
        let class_loader = self.class_loader_with(&local_classes, &file_use_map, &file_namespace);
        let function_loader = self.function_loader_with(None, &file_use_map, &file_namespace);

        infer_return_type(
            content,
            func_line,
            &local_classes,
            &class_loader,
            Some(&function_loader),
            self_as_marker,
        )
    }
}

// ── Shared return-type inference ────────────────────────────────────────────

/// Infer the return type of a function by walking all `return`
/// statements reachable from its body (not crossing into nested
/// closures/arrow functions, which have their own return types).
///
/// Every returned expression is resolved through
/// [`resolve_expression_type`] — the same RHS resolution pipeline used
/// by hover, completion, and diagnostics — so literals, array shapes,
/// `new` instantiations, method calls, and variables are all handled
/// consistently and any future improvement to that pipeline benefits
/// this inference automatically.
///
/// Returns an [`InferredReturnType`] that separates the native PHP
/// type hint from the richer effective type.  When they differ (e.g.
/// `list<string>` vs `array`), the caller should add a `@return` tag.
///
/// When `self_as_marker` is `true`, a `return $this;` statement yields
/// the self-like marker `$this` instead of resolving to the concrete
/// enclosing class.  The type engine needs this so that a fluent method
/// inherited from a trait maps to the class the method is *called on*,
/// not the trait that lexically declares it.  Code actions that write a
/// concrete return type or docblock pass `false`.
///
/// This is the shared core used by:
/// - `Backend::infer_return_type_for_function` (PHPStan code actions)
/// - `enrichment_return_type` (Generate / Update PHPDoc)
pub(crate) fn infer_return_type(
    content: &str,
    func_line: usize,
    local_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    function_loader: FunctionLoader<'_>,
    self_as_marker: bool,
) -> Option<InferredReturnType> {
    let lines: Vec<&str> = content.lines().collect();
    if func_line >= lines.len() {
        return None;
    }

    // Find the function's opening brace, and a byte offset guaranteed
    // to fall inside the body (the brace itself), so the AST lookup
    // below can locate the enclosing function/method regardless of
    // how its body is formatted (single-line, multi-line, nested
    // control flow, comments, etc.).
    let brace_line = find_open_brace_from_declaration(&lines, func_line)?;
    let brace_col = lines[brace_line].find('{')?;
    let body_probe_offset = (line_start_byte_offset(content, brace_line) + brace_col) as u32;

    // Find the enclosing class at the function line offset.
    let func_offset = line_start_byte_offset(content, func_line) as u32;
    let enclosing_class = local_classes
        .iter()
        .find(|c| {
            !c.name.starts_with("__anonymous@")
                && func_offset >= c.start_offset
                && func_offset <= c.end_offset
        })
        .map(|c| ClassInfo::clone(c))
        .unwrap_or_default();

    let (return_types, has_bare_return, has_return_with_value) =
        with_parsed_program(content, "fix_return_type_infer", |program, _content| {
            let body_stmts = crate::code_actions::extract_function::find_enclosing_body_statements(
                &program.statements,
                body_probe_offset,
            );

            let mut returns: Vec<(Option<&Expression<'_>>, usize, usize)> = Vec::new();
            collect_returns(body_stmts.into_iter(), &mut returns);

            let mut return_types: Vec<PhpType> = Vec::new();
            let mut has_bare_return = false;
            let mut has_return_with_value = false;

            for (maybe_expr, start, _end) in returns {
                let Some(expr) = maybe_expr else {
                    has_bare_return = true;
                    continue;
                };
                has_return_with_value = true;

                // `return $this;` is a fluent self-return.  Yield the
                // self-like marker so the caller maps it to the actual
                // receiver class rather than the class that lexically
                // declares the method (which for a trait method is the
                // trait, not the using class).
                if self_as_marker && is_this_variable(expr) {
                    return_types.push(PhpType::this());
                    continue;
                }

                let ctx = VarResolutionCtx {
                    var_name: "",
                    top_level_scope: None,
                    current_class: &enclosing_class,
                    all_classes: local_classes,
                    content,
                    cursor_offset: start as u32,
                    class_loader,
                    loaders: Loaders::with_function(function_loader),
                    resolved_class_cache: None,
                    enclosing_return_type: None,
                    branch_aware: true,
                    match_arm_narrowing: HashMap::new(),
                    scope_var_resolver: None,
                };

                let ty = resolve_expression_type(expr, &ctx).unwrap_or_else(PhpType::mixed);
                let ty = ty.resolve_names(&|name: &str| {
                    if let Some(cls) = class_loader(name) {
                        cls.fqn().to_string()
                    } else {
                        name.to_string()
                    }
                });
                return_types.push(ty);
            }

            (return_types, has_bare_return, has_return_with_value)
        });

    if !has_return_with_value && !has_bare_return {
        return Some(InferredReturnType {
            native: PhpType::void(),
            effective: None,
        });
    }

    if return_types.is_empty() && has_bare_return {
        return Some(InferredReturnType {
            native: PhpType::void(),
            effective: None,
        });
    }

    // Deduplicate types structurally (no string round-trip).
    let mut deduped: Vec<PhpType> = Vec::with_capacity(return_types.len());
    for ty in &return_types {
        if !deduped.iter().any(|existing| existing.equivalent(ty)) {
            deduped.push(ty.clone());
        }
    }

    if has_bare_return {
        let has_null = deduped.iter().any(|t| t.is_null());
        if !has_null {
            deduped.push(PhpType::null());
        }
    }

    let effective = if deduped.len() == 1 {
        deduped.into_iter().next().unwrap()
    } else if deduped.len() <= 3 {
        PhpType::Union(deduped)
    } else {
        return None;
    };

    // Convert effective type → native PHP type hint.
    let native = effective
        .to_native_hint_typed()
        .unwrap_or_else(PhpType::mixed);

    let needs_docblock = !native.equivalent(&effective);
    Some(InferredReturnType {
        native,
        effective: if needs_docblock {
            Some(effective)
        } else {
            None
        },
    })
}

/// Whether `expr` is the bare `$this` variable.
fn is_this_variable(expr: &Expression<'_>) -> bool {
    matches!(expr, Expression::Variable(Variable::Direct(dv)) if bytes_to_str(dv.name) == "$this")
}

/// Infer a `@return` type string for a function whose signature is
/// at `position` in `content`.
///
/// Returns `Some("list<string>")` when the body analysis produces a
/// type richer than the native hint, or `None` when inference fails
/// or the native type already captures the full information.
///
/// This is the entry point for docblock generation (`enrichment_plain`
/// replacement for `@return`) — it finds the function line from the
/// position and delegates to [`infer_return_type`].
pub(crate) fn enrichment_return_type(
    content: &str,
    position: Position,
    local_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    function_loader: FunctionLoader<'_>,
) -> Option<PhpType> {
    // The position is on or near the docblock / function signature.
    // Search forward from that line to find the `function` keyword.
    let lines: Vec<&str> = content.lines().collect();
    let start = position.line as usize;
    let end = (start + 10).min(lines.len());
    let func_line =
        (start..end).find(|&i| lines[i].contains("function ") || lines[i].contains("function("))?;

    let inferred = infer_return_type(
        content,
        func_line,
        local_classes,
        class_loader,
        function_loader,
        // Docblock generation wants a concrete written type, not a `$this`
        // marker, so resolve `return $this` to the enclosing class.
        false,
    )?;

    // Return the effective type if it's richer than the native hint,
    // otherwise return the native type (which may still be useful for
    // callers that want any inferred type, e.g. `void`).
    Some(inferred.effective.unwrap_or(inferred.native))
}
