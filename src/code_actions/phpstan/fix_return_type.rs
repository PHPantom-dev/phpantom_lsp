//! "Fix return type" code actions for PHPStan return-type diagnostics.
//!
//! Handles four PHPStan identifiers:
//!
//! - **`return.void`** — a `void` function returns an expression.
//!   Two fixes: remove the return statement (keeping the expression
//!   as a standalone statement followed by `return;`), or change the
//!   return type to match the actual value.
//! - **`return.empty`** — a non-void function has a bare `return;`.
//!   Fix: change the return type to `void` and remove `@return`.
//! - **`return.type`** — the return type doesn't match what the
//!   function actually returns.  Single fix: "Update return type"
//!   which updates the native type hint (to the base type, stripping
//!   generics) and updates or creates a `@return` docblock tag with
//!   the full type.  Not marked as preferred since the right fix
//!   might be to change the code rather than the signature.  The
//!   exact edits are deferred to Phase 2 to keep Phase 1 cheap.
//! - **`missingType.return`** — no return type specified.
//!   Fix: add a return type hint.  The type is inferred from the
//!   function body by scanning return statements for literals,
//!   variable types, and `new ClassName()` expressions.
//!
//! **Code action kind:** `quickfix`.
//!
//! ## Two-phase resolve
//!
//! Phase 1 (`collect_fix_return_type_actions`) validates that the
//! action is applicable and emits a lightweight `CodeAction` with a
//! `data` payload but no `edit`.  Phase 2 (`resolve_fix_return_type`)
//! recomputes the workspace edit on demand when the user picks the
//! action.

use std::collections::HashMap;
use std::sync::Arc;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::code_actions::phpstan::add_iterable_type::{
    find_function_docblock, find_function_keyword_line as find_func_keyword_line,
};
use crate::code_actions::{CodeActionData, make_code_action_data};
use crate::completion::resolver::Loaders;
use crate::completion::variable::resolution::resolve_variable_types;
use crate::docblock::type_strings::split_type_token;
use crate::php_type::PhpType;
use crate::types::{ClassInfo, FunctionLoader, ResolvedType};
use crate::util::{
    find_brace_match_line, find_semicolon_balanced, line_start_byte_offset, ranges_overlap,
};

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

// ── PHPStan identifiers ─────────────────────────────────────────────────────

/// PHPStan identifier for "void function returns a value".
const RETURN_VOID_ID: &str = "return.void";

/// PHPStan identifier for "non-void function has empty return".
const RETURN_EMPTY_ID: &str = "return.empty";

/// PHPStan identifier for "return type doesn't match actual return".
const RETURN_TYPE_ID: &str = "return.type";

/// PHPStan identifier for "no return type specified".
const MISSING_TYPE_RETURN_ID: &str = "missingType.return";

/// Action kind string for the strip-expression fix (return.void).
const ACTION_KIND_STRIP_EXPR: &str = "phpstan.fixReturnType.stripExpr";

/// Action kind string for changing the return type to match the actual
/// return value (return.void only — simple types without generics).
const ACTION_KIND_CHANGE_TYPE_TO_ACTUAL: &str = "phpstan.fixReturnType.changeTypeToActual";

/// Action kind string for the change-return-type-to-void fix (return.empty).
const ACTION_KIND_CHANGE_TYPE: &str = "phpstan.fixReturnType.changeType";

/// Action kind string for adding a missing return type hint.
const ACTION_KIND_ADD_TYPE: &str = "phpstan.fixReturnType.addType";

/// Action kind string for the unified "Update return type" fix
/// (return.type).  Updates both the native type hint and the `@return`
/// docblock tag in a single action.
const ACTION_KIND_UPDATE_RETURN_TYPE: &str = "phpstan.fixReturnType.updateReturnType";

/// Message fragment that identifies a `return.void` diagnostic.
const RETURN_VOID_MSG_SUFFIX: &str = "but should not return anything.";

/// Message fragment that identifies a `return.empty` diagnostic.
const RETURN_EMPTY_MSG_FRAGMENT: &str = "but empty return statement found.";

// ── Backend methods ─────────────────────────────────────────────────────────

impl Backend {
    /// Collect code actions for PHPStan `return.void`, `return.empty`,
    /// `return.type`, and `missingType.return` diagnostics.
    pub(crate) fn collect_fix_return_type_actions(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionOrCommand>,
    ) {
        let phpstan_diags: Vec<Diagnostic> = {
            let cache = self.phpstan_last_diags.lock();
            cache.get(uri).cloned().unwrap_or_default()
        };

        for diag in &phpstan_diags {
            if !ranges_overlap(&diag.range, &params.range) {
                continue;
            }

            let identifier = match &diag.code {
                Some(NumberOrString::String(s)) => s.as_str(),
                _ => continue,
            };

            let diag_line = diag.range.start.line as usize;

            match identifier {
                RETURN_VOID_ID => {
                    if !diag.message.ends_with(RETURN_VOID_MSG_SUFFIX) {
                        continue;
                    }

                    // Verify the strip-expression fix is applicable.
                    if build_strip_return_expr_edit(content, diag_line).is_none() {
                        continue;
                    }

                    // ── Fix 1: Strip return expression ──────────────
                    let extra = serde_json::json!({
                        "diagnostic_line": diag_line,
                        "identifier": RETURN_VOID_ID,
                    });

                    out.push(CodeActionOrCommand::CodeAction(CodeAction {
                        title: "Remove return statement".to_string(),
                        kind: Some(CodeActionKind::QUICKFIX),
                        diagnostics: Some(vec![diag.clone()]),
                        edit: None,
                        command: None,
                        is_preferred: Some(false),
                        disabled: None,
                        data: Some(make_code_action_data(
                            ACTION_KIND_STRIP_EXPR,
                            uri,
                            &params.range,
                            extra,
                        )),
                    }));

                    // ── Fix 2: Change return type to match actual ───
                    // Extract the actual type from the message:
                    // "... returns {actual} but should not return anything."
                    // Skip when the actual type is `null` — returning null
                    // from a void function is not a type mismatch, it's
                    // just a habit.  The "Remove return statement" fix above
                    // handles it.
                    if let Some(actual_type) = extract_actual_type(&diag.message)
                        && !actual_type.is_null()
                    {
                        // Verify the change-type fix is applicable (the
                        // function has a return type that can be replaced).
                        if build_change_return_type_edits_to(content, diag_line, &actual_type)
                            .is_some()
                        {
                            let actual_str = actual_type.to_string();
                            let extra = serde_json::json!({
                                "diagnostic_line": diag_line,
                                "identifier": RETURN_VOID_ID,
                                "actual_type": actual_str,
                            });

                            out.push(CodeActionOrCommand::CodeAction(CodeAction {
                                title: format!("Change return type to {}", actual_str),
                                kind: Some(CodeActionKind::QUICKFIX),
                                diagnostics: Some(vec![diag.clone()]),
                                edit: None,
                                command: None,
                                is_preferred: Some(true),
                                disabled: None,
                                data: Some(make_code_action_data(
                                    ACTION_KIND_CHANGE_TYPE_TO_ACTUAL,
                                    uri,
                                    &params.range,
                                    extra,
                                )),
                            }));
                        }
                    }
                }
                RETURN_EMPTY_ID => {
                    if !diag.message.contains(RETURN_EMPTY_MSG_FRAGMENT) {
                        continue;
                    }

                    // Verify the fix is applicable.
                    if build_change_return_type_edits_to(content, diag_line, &PhpType::void())
                        .is_none()
                    {
                        continue;
                    }

                    let title = "Change return type to void".to_string();

                    let extra = serde_json::json!({
                        "diagnostic_line": diag_line,
                        "identifier": RETURN_EMPTY_ID,
                    });

                    let data =
                        make_code_action_data(ACTION_KIND_CHANGE_TYPE, uri, &params.range, extra);

                    out.push(CodeActionOrCommand::CodeAction(CodeAction {
                        title,
                        kind: Some(CodeActionKind::QUICKFIX),
                        diagnostics: Some(vec![diag.clone()]),
                        edit: None,
                        command: None,
                        is_preferred: Some(true),
                        disabled: None,
                        data: Some(data),
                    }));
                }
                RETURN_TYPE_ID => {
                    // "Method Foo::bar() should return {expected} but returns {actual}."
                    // Just verify the message is parseable — defer all
                    // computation to Phase 2.
                    if extract_return_type_actual(&diag.message).is_none() {
                        continue;
                    }

                    let extra = serde_json::json!({
                        "diagnostic_line": diag_line,
                        "identifier": RETURN_TYPE_ID,
                        "message": &diag.message,
                    });

                    out.push(CodeActionOrCommand::CodeAction(CodeAction {
                        title: "Update return type".to_string(),
                        kind: Some(CodeActionKind::QUICKFIX),
                        diagnostics: Some(vec![diag.clone()]),
                        edit: None,
                        command: None,
                        is_preferred: Some(false),
                        disabled: None,
                        data: Some(make_code_action_data(
                            ACTION_KIND_UPDATE_RETURN_TYPE,
                            uri,
                            &params.range,
                            extra,
                        )),
                    }));
                }
                MISSING_TYPE_RETURN_ID => {
                    // "Method Foo::bar() has no return type specified."
                    // Defer type inference to the resolve phase — it can
                    // be expensive and the collect phase runs on every
                    // cursor move.  Just validate that the function has
                    // no return type yet.
                    let lines: Vec<&str> = content.lines().collect();

                    let brace_line = match find_open_brace_from_declaration(&lines, diag_line) {
                        Some(l) => l,
                        None => continue,
                    };
                    let (paren_line, paren_col) =
                        match find_close_paren_before_brace(&lines, brace_line) {
                            Some(p) => p,
                            None => continue,
                        };

                    // Check there is no existing return type.
                    if has_return_type_between(&lines, paren_line, paren_col, brace_line) {
                        continue;
                    }

                    let extra = serde_json::json!({
                        "diagnostic_line": diag_line,
                        "identifier": MISSING_TYPE_RETURN_ID,
                    });

                    out.push(CodeActionOrCommand::CodeAction(CodeAction {
                        title: "Add return type".to_string(),
                        kind: Some(CodeActionKind::QUICKFIX),
                        diagnostics: Some(vec![diag.clone()]),
                        edit: None,
                        command: None,
                        is_preferred: Some(true),
                        disabled: None,
                        data: Some(make_code_action_data(
                            ACTION_KIND_ADD_TYPE,
                            uri,
                            &params.range,
                            extra,
                        )),
                    }));
                }
                _ => continue,
            }
        }
    }

    /// Infer the return type of the function at `func_line` by scanning
    /// all return statements in the body.
    ///
    /// For simple literals (`return 1;`, `return 'hello';`, `return new Foo()`)
    /// the type is inferred syntactically. For `$variable` returns, the
    /// full variable-resolution pipeline is used. All other expressions
    /// (method calls, function calls, complex expressions) produce `mixed`.
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
        let function_loader = self.function_loader_with(&file_use_map, &file_namespace);

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

/// Infer the return type of a function by scanning all `return`
/// statements in the body.
///
/// For simple literals (`return 1;`, `return 'hello';`, `return new Foo()`)
/// the type is inferred syntactically.  For `$variable` returns, the
/// full variable-resolution pipeline is used.  All other expressions
/// (method calls, function calls, complex expressions) produce `mixed`.
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

    // Find the function body boundaries.
    let brace_line = find_open_brace_from_declaration(&lines, func_line)?;

    // Find the closing `}` that matches the `{` on `brace_line`.
    let body_end = find_brace_match_line(&lines, brace_line, |d| d == 0)?;

    // Find the enclosing class at the function line offset.
    let func_offset = content
        .lines()
        .take(func_line)
        .map(|l| l.len() + 1)
        .sum::<usize>() as u32;
    let enclosing_class = local_classes
        .iter()
        .find(|c| {
            !c.name.starts_with("__anonymous@")
                && func_offset >= c.start_offset
                && func_offset <= c.end_offset
        })
        .map(|c| ClassInfo::clone(c))
        .unwrap_or_default();

    // Scan return statements and resolve their types.
    let mut return_types: Vec<PhpType> = Vec::new();
    let mut has_bare_return = false;
    let mut has_return_with_value = false;

    // Single-line function body: `function foo() { return new Bar(); }`
    // The normal loop (skip(brace_line+1)..take(body_end)) would iterate
    // zero times because brace_line == body_end.  Extract the content
    // between the first `{` and last `}` and scan it directly.
    if brace_line == body_end
        && let line = lines[brace_line]
        && let Some(open) = line.find('{')
        && let Some(close) = line.rfind('}')
        && close > open + 1
    {
        let inner = line[open + 1..close].trim();
        if inner == "return;" || inner.is_empty() {
            has_bare_return = !inner.is_empty();
        } else if let Some(rest) = inner.strip_prefix("return ") {
            let expr = rest.strip_suffix(';').unwrap_or(rest).trim();
            has_return_with_value = true;
            if self_as_marker && expr == "$this" {
                return_types.push(PhpType::this());
            } else if let Some(t) = infer_type_from_literal(expr) {
                let resolved = t.resolve_names(&|name: &str| {
                    if let Some(cls) = class_loader(name) {
                        cls.fqn().to_string()
                    } else {
                        name.to_string()
                    }
                });
                return_types.push(resolved);
            } else {
                return_types.push(PhpType::mixed());
            }
        }
    }

    // Track nested closure/anonymous-function depth so we skip their
    // return statements (they belong to a different scope) while still
    // capturing returns inside control structures (if/for/while/switch/
    // try) which are in the same scope.
    //
    // `closure_depth` only increments when a line contains a `function`
    // or `fn` keyword before an opening `{`.  Plain `{` from control
    // structures does not change it.
    let mut closure_depth: i32 = 0;

    for (line_idx, line) in lines.iter().enumerate().take(body_end).skip(brace_line + 1) {
        let trimmed = line.trim();

        // Detect nested closure / anonymous function openings.
        // A line like `$f = function () {` or `fn() => {` introduces
        // a new function scope whose returns we must ignore.
        let has_fn_keyword = trimmed.contains("function ")
            || trimmed.contains("function(")
            || trimmed.starts_with("fn ")
            || trimmed.starts_with("fn(")
            || trimmed.contains(" fn(")
            || trimmed.contains(" fn ");

        for ch in line.chars() {
            match ch {
                '{' => {
                    if has_fn_keyword && closure_depth == 0 {
                        // First `{` on a line with a function keyword
                        // opens a nested scope.
                        closure_depth += 1;
                    } else if closure_depth > 0 {
                        // Already inside a nested closure — track depth
                        // so we know when we leave it.
                        closure_depth += 1;
                    }
                    // Plain `{` at closure_depth == 0 from control
                    // structures: do nothing.
                }
                '}' if closure_depth > 0 => {
                    closure_depth -= 1;
                }
                _ => {}
            }
        }

        // Skip return statements inside nested closures / anonymous fns.
        if closure_depth > 0 {
            continue;
        }

        // Skip comments.
        if trimmed.starts_with("//") || trimmed.starts_with('*') || trimmed.starts_with("/*") {
            continue;
        }

        if trimmed == "return;" {
            has_bare_return = true;
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("return ") {
            let rest = rest.trim();
            if rest == ";" {
                has_bare_return = true;
                continue;
            }
            has_return_with_value = true;

            // Strip trailing `;`
            let expr = rest.strip_suffix(';').unwrap_or(rest).trim();

            // `return $this;` is a fluent self-return.  Yield the self-like
            // marker so the caller maps it to the actual receiver class
            // rather than the class that lexically declares the method
            // (which for a trait method is the trait, not the using class).
            if self_as_marker && expr == "$this" {
                return_types.push(PhpType::this());
                continue;
            }

            // Try syntax-level inference first (cheap).
            if let Some(t) = infer_type_from_literal(expr) {
                // Resolve short class names to FQN via the class loader
                // so that `new Foo(…)` produces a fully-qualified type.
                let resolved = t.resolve_names(&|name: &str| {
                    if let Some(cls) = class_loader(name) {
                        cls.fqn().to_string()
                    } else {
                        name.to_string()
                    }
                });
                return_types.push(resolved);
                continue;
            }

            // Fall back to the variable/expression resolver.
            // Compute byte offset of the expression for resolution.
            let line_start = line_start_byte_offset(content, line_idx);
            let expr_offset_in_line = line.find("return ").unwrap_or(0) + "return ".len();
            let expr_offset = (line_start + expr_offset_in_line) as u32;

            // Try variable resolution for `$var` expressions.
            if expr.starts_with('$') && !expr.contains(' ') {
                let results = resolve_variable_types(
                    expr,
                    &enclosing_class,
                    local_classes,
                    content,
                    expr_offset,
                    class_loader,
                    Loaders::with_function(function_loader),
                );
                let joined = ResolvedType::types_joined(&results);
                if !joined.is_mixed() {
                    return_types.push(joined);
                    continue;
                }
            }

            // For other expressions, fall back to `mixed`.
            return_types.push(PhpType::mixed());
        }
    }

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

impl Backend {
    /// Resolve a "Fix return type" code action by computing the full
    /// workspace edit.  Dispatches on the `action_kind` stored in the
    /// data payload.
    pub(crate) fn resolve_fix_return_type(
        &self,
        data: &CodeActionData,
        content: &str,
    ) -> Option<WorkspaceEdit> {
        let extra = &data.extra;
        let diag_line = extra.get("diagnostic_line")?.as_u64()? as usize;

        let doc_uri: Url = data.uri.parse().ok()?;

        match data.action_kind.as_str() {
            ACTION_KIND_STRIP_EXPR => {
                let edit = build_strip_return_expr_edit(content, diag_line)?;
                let mut changes = HashMap::new();
                changes.insert(doc_uri, vec![edit]);
                Some(WorkspaceEdit {
                    changes: Some(changes),
                    document_changes: None,
                    change_annotations: None,
                })
            }
            ACTION_KIND_CHANGE_TYPE_TO_ACTUAL => {
                let actual_type_str = extra.get("actual_type")?.as_str()?;
                let actual_type = PhpType::parse(actual_type_str);
                let edits = build_change_return_type_edits_to(content, diag_line, &actual_type)?;
                let mut changes = HashMap::new();
                changes.insert(doc_uri, edits);
                Some(WorkspaceEdit {
                    changes: Some(changes),
                    document_changes: None,
                    change_annotations: None,
                })
            }
            ACTION_KIND_UPDATE_RETURN_TYPE => {
                let diag_msg = extra.get("message")?.as_str()?;
                let tip_type = extract_return_type_actual(diag_msg)?;

                // Run our own inference and compare to the current
                // declaration.  If they differ, our inference spotted
                // the mismatch — use it (it already has the correct
                // native/effective split, e.g. native=`array`,
                // effective=`list<int>`).  If they agree, we can't
                // see the problem ourselves — trust the PHPStan tip.
                //
                // The diagnostic line is a return statement inside the
                // body.  Walk backward to find the function declaration
                // line so the inference engine can locate the full body.
                let lines: Vec<&str> = content.lines().collect();
                let brace_line = find_function_open_brace_line(&lines, diag_line)?;
                let (paren_line, _) = find_close_paren_before_brace(&lines, brace_line)?;
                let func_line = find_func_keyword_line(&lines, paren_line)?;

                let our = self.infer_return_type_for_function(&data.uri, content, func_line, false);
                let current = read_current_return_type(content, diag_line);

                let edits = if should_use_own_inference(&our, &current) {
                    let inferred = our?;
                    build_update_return_type_edits_split(
                        content,
                        diag_line,
                        &inferred.native,
                        inferred.effective.as_ref(),
                    )?
                } else {
                    // Trust the PHPStan tip.
                    build_update_return_type_edits(content, diag_line, &tip_type)?
                };

                let mut changes = HashMap::new();
                changes.insert(doc_uri, edits);
                Some(WorkspaceEdit {
                    changes: Some(changes),
                    document_changes: None,
                    change_annotations: None,
                })
            }
            ACTION_KIND_CHANGE_TYPE => {
                let void = PhpType::void();
                let edits = build_change_return_type_edits_to(content, diag_line, &void)?;
                let mut changes = HashMap::new();
                changes.insert(doc_uri, edits);
                Some(WorkspaceEdit {
                    changes: Some(changes),
                    document_changes: None,
                    change_annotations: None,
                })
            }
            ACTION_KIND_ADD_TYPE => {
                // Infer the type now (deferred from collect phase).
                let inferred =
                    self.infer_return_type_for_function(&data.uri, content, diag_line, false)?;

                let native_str = inferred.native.to_string();

                let lines: Vec<&str> = content.lines().collect();
                let brace_line = find_open_brace_from_declaration(&lines, diag_line)?;
                let (paren_line, paren_col) = find_close_paren_before_brace(&lines, brace_line)?;

                let mut edits = Vec::new();

                // Insert `: native_type` after the closing paren.
                edits.push(TextEdit {
                    range: Range {
                        start: Position::new(paren_line as u32, (paren_col + 1) as u32),
                        end: Position::new(paren_line as u32, (paren_col + 1) as u32),
                    },
                    new_text: format!(": {}", native_str),
                });

                // When the effective type is richer than the native hint,
                // add a `@return` docblock tag.
                if let Some(ref eff_type) = inferred.effective {
                    let eff = eff_type.to_string();
                    let func_line = find_func_keyword_line(&lines, paren_line).unwrap_or(diag_line);
                    let docblock_info = find_function_docblock(&lines, func_line);

                    if docblock_info.has_docblock {
                        if !docblock_info.has_return_tag {
                            // Insert @return into the existing docblock.
                            let doc_end = docblock_info.doc_end_line;
                            let close_line = lines[doc_end];

                            if docblock_info.doc_start_line == doc_end {
                                // Single-line docblock: convert to multi-line.
                                let trimmed = close_line.trim();
                                let inner = trimmed
                                    .strip_prefix("/**")
                                    .and_then(|s| s.strip_suffix("*/"))
                                    .map(|s| s.trim())
                                    .unwrap_or("");

                                let indent = &docblock_info.indent;
                                let mut new_doc = format!("{}/**\n", indent);
                                if !inner.is_empty() {
                                    new_doc.push_str(&format!("{} * {}\n", indent, inner));
                                    new_doc.push_str(&format!("{} *\n", indent));
                                }
                                new_doc.push_str(&format!("{} * @return {}\n", indent, eff));
                                new_doc.push_str(&format!("{} */", indent));

                                edits.push(TextEdit {
                                    range: Range {
                                        start: Position::new(doc_end as u32, 0),
                                        end: Position::new(doc_end as u32, close_line.len() as u32),
                                    },
                                    new_text: new_doc,
                                });
                            } else {
                                // Multi-line docblock: insert @return before `*/`.
                                let indent = &docblock_info.indent;

                                let prev_line = if doc_end > docblock_info.doc_start_line {
                                    lines[doc_end - 1].trim()
                                } else {
                                    ""
                                };
                                let prev_trimmed = prev_line.trim_start_matches('*').trim();
                                let needs_separator = !prev_trimmed.is_empty()
                                    && !prev_trimmed.starts_with("@return")
                                    && !prev_trimmed.starts_with("@throws")
                                    && prev_trimmed.starts_with('@');

                                let mut insert_text = String::new();
                                if needs_separator {
                                    insert_text.push_str(&format!("{} *\n", indent));
                                }
                                insert_text.push_str(&format!("{} * @return {}\n", indent, eff));

                                edits.push(TextEdit {
                                    range: Range {
                                        start: Position::new(doc_end as u32, 0),
                                        end: Position::new(doc_end as u32, 0),
                                    },
                                    new_text: insert_text,
                                });
                            }
                        }
                        // If the docblock already has a @return tag, we
                        // don't overwrite it — the user intentionally
                        // wrote it.
                    } else {
                        // No existing docblock — create one.
                        let indent = &docblock_info.indent;
                        let new_doc = format!(
                            "{}/**\n{} * @return {}\n{} */\n",
                            indent, indent, eff, indent
                        );
                        edits.push(TextEdit {
                            range: Range {
                                start: Position::new(func_line as u32, 0),
                                end: Position::new(func_line as u32, 0),
                            },
                            new_text: new_doc,
                        });
                    }
                }

                let mut changes = HashMap::new();
                changes.insert(doc_uri, edits);
                Some(WorkspaceEdit {
                    changes: Some(changes),
                    document_changes: None,
                    change_annotations: None,
                })
            }
            _ => None,
        }
    }
}

// ── Edit builders ───────────────────────────────────────────────────────────

/// Build a `TextEdit` that fixes `return {expr};` in a void function.
///
/// The replacement depends on context:
///
/// - **`return null;`** → `return;` (null is not a meaningful value).
/// - **All other expressions** → `{expr};\n{indent}return;` (keep
///   the expression as a standalone statement and add a bare
///   `return;` on the next line).
///
/// When the return is the last statement before the function's closing
/// `}`, the bare `return;` is omitted since it would be redundant.
///
/// Handles multiline return expressions by scanning forward from the
/// `return` keyword to the matching `;`, respecting string literals and
/// parenthesis nesting.
fn build_strip_return_expr_edit(content: &str, diag_line: usize) -> Option<TextEdit> {
    let lines: Vec<&str> = content.lines().collect();
    if diag_line >= lines.len() {
        return None;
    }

    let line_text = lines[diag_line];

    // Find `return ` (with trailing space) on the diagnostic line.
    let return_col = line_text.find("return ")?;

    // Verify this is not `return;` (no expression).
    let after_return = &line_text[return_col + "return".len()..];
    let trimmed = after_return.trim();
    if trimmed == ";" {
        // Already a bare return — nothing to fix.
        return None;
    }

    // Compute the byte offset within `content` where this line starts.
    let line_start_byte: usize = lines[..diag_line]
        .iter()
        .map(|l| l.len() + 1) // +1 for newline
        .sum();

    // The return statement starts at `return` keyword.
    let return_byte = line_start_byte + return_col;

    // Walk forward from after `return` to find the terminating `;`,
    // respecting string literals and balanced parentheses.
    let after_keyword_byte = return_byte + "return".len();
    let semi_offset = find_semicolon_balanced(&content[after_keyword_byte..])?;
    let semi_byte = after_keyword_byte + semi_offset;

    // Build the replacement range: from `return` keyword through `;`.
    let stmt_end_byte = semi_byte + 1;

    // Compute line/col for the start (the `return` keyword).
    let start_line = diag_line as u32;
    let start_char = return_col as u32;

    // Compute line/col for the end (after `;`).
    let end_line = content[..stmt_end_byte].matches('\n').count() as u32;
    let end_line_start = content[..stmt_end_byte]
        .rfind('\n')
        .map(|p| p + 1)
        .unwrap_or(0);
    let end_char = (stmt_end_byte - end_line_start) as u32;

    // Extract the expression text (between `return ` and `;`).
    let expr_start = return_byte + "return ".len();
    let expr_text = content[expr_start..semi_byte].trim();

    // Case 1: `return null;` → just replace with `return;`.
    if expr_text == "null" {
        return Some(TextEdit {
            range: Range {
                start: Position::new(start_line, start_char),
                end: Position::new(end_line, end_char),
            },
            new_text: "return;".to_string(),
        });
    }

    // Capture the indentation of the return line.
    let indent = &line_text[..return_col];

    // Check whether this return is the last statement in the function
    // body.  If the only thing between the `;` and the function's
    // closing `}` is whitespace, the `return;` is redundant.
    let needs_bare_return = !is_last_statement_in_function(content, stmt_end_byte);

    let new_text = if needs_bare_return {
        format!("{};\n{}return;", expr_text, indent)
    } else {
        format!("{};", expr_text)
    };

    Some(TextEdit {
        range: Range {
            start: Position::new(start_line, start_char),
            end: Position::new(end_line, end_char),
        },
        new_text,
    })
}

/// Build a list of `TextEdit`s that change the enclosing function's
/// return type to `target_type` and, when the target is `void`,
/// optionally remove the `@return` docblock tag.
///
/// Returns `None` if the enclosing function cannot be found or its
/// return type already matches `target_type`.
fn build_change_return_type_edits_to(
    content: &str,
    diag_line: usize,
    target_type: &PhpType,
) -> Option<Vec<TextEdit>> {
    let lines: Vec<&str> = content.lines().collect();
    if diag_line >= lines.len() {
        return None;
    }

    let mut edits = Vec::new();

    // ── Step 1: Find the opening `{` of the function body ───────────
    // The diagnostic is on a `return` statement inside the body, so
    // search backward to find the enclosing function's opening brace.
    let brace_line = find_function_open_brace_line(&lines, diag_line)?;

    // ── Step 2: Find the `)` that closes the parameter list ─────────
    let (paren_line, paren_col) = find_close_paren_before_brace(&lines, brace_line)?;

    // ── Step 3: Find the return type hint between `)` and `{` ───────
    let target_str = target_type.to_string();
    let type_edit = find_return_type_edit(&lines, paren_line, paren_col, brace_line, &target_str)?;
    edits.push(type_edit);

    // ── Step 4: Find the function signature line ────────────────────
    let func_line = find_func_keyword_line(&lines, paren_line)?;

    // ── Step 5: Remove @return from docblock when target is void ────
    if target_type.is_void()
        && let Some(return_tag_edit) = find_and_remove_return_tag(&lines, func_line)
    {
        edits.push(return_tag_edit);
    }

    Some(edits)
}

/// The current return type declaration as read from the source text.
///
/// Combines the native type hint (`: array`) with the `@return` tag
/// type (if any) into a single effective type string that can be
/// compared against our inference result.
struct CurrentReturnType {
    /// The native type hint after `:`, e.g. `array`, `int`.
    /// `None` when the function has no return type declaration.
    native: Option<PhpType>,
    /// The `@return` tag type, e.g. `array<int, string>`.
    /// `None` when there is no docblock or no `@return` tag.
    docblock: Option<PhpType>,
}

/// Read the current native return type and `@return` tag type from the
/// source text around `diag_line` (a return statement inside the body).
fn read_current_return_type(content: &str, diag_line: usize) -> CurrentReturnType {
    let lines: Vec<&str> = content.lines().collect();
    if diag_line >= lines.len() {
        return CurrentReturnType {
            native: None,
            docblock: None,
        };
    }

    let brace_line = match find_function_open_brace_line(&lines, diag_line) {
        Some(l) => l,
        None => {
            return CurrentReturnType {
                native: None,
                docblock: None,
            };
        }
    };
    let (paren_line, paren_col) = match find_close_paren_before_brace(&lines, brace_line) {
        Some(p) => p,
        None => {
            return CurrentReturnType {
                native: None,
                docblock: None,
            };
        }
    };

    // ── Native type hint ────────────────────────────────────────────
    let between = gather_between_paren_and_brace(&lines, paren_line, paren_col, brace_line);
    let native = between.find(':').map(|colon_pos| {
        let after_colon = &between[colon_pos + 1..];
        let type_start = after_colon.find(|c: char| !c.is_whitespace()).unwrap_or(0);
        let type_text = &after_colon[type_start..];
        let type_len = type_text
            .find(|c: char| c.is_whitespace() || c == '{')
            .unwrap_or(type_text.len());
        PhpType::parse(&type_text[..type_len])
    });

    // ── @return tag type ────────────────────────────────────────────
    let func_line = find_func_keyword_line(&lines, paren_line);
    let docblock = func_line.and_then(|fl| {
        let info = find_function_docblock(&lines, fl);
        let tag_line = info.return_tag_line?;
        let line_text = lines[tag_line];
        let at_pos = line_text.find("@return")?;
        let after = &line_text[at_pos + "@return".len()..];
        let trimmed = after.trim_start();
        if trimmed.is_empty() {
            return None;
        }
        let (type_token, _remainder) = split_type_token(trimmed);
        Some(PhpType::parse(type_token))
    });

    CurrentReturnType { native, docblock }
}

/// Decide whether to use our own inference or fall back to the
/// PHPStan tip.
///
/// Returns `true` when our inference disagrees with the current
/// declaration (native hint + `@return` tag) — meaning we can see
/// the mismatch ourselves and our type is likely more specific
/// (e.g. `list<int>` vs PHPStan's `array<int, int>`).
///
/// Returns `false` when inference failed or agrees with the
/// declaration — we can't see the bug, so the caller should trust
/// the PHPStan tip.
fn should_use_own_inference(our: &Option<InferredReturnType>, current: &CurrentReturnType) -> bool {
    let Some(inferred) = our else {
        return false;
    };

    // The effective type our inference would write (prefers the rich
    // docblock type, falls back to the native hint).
    let our_effective = inferred.effective.as_ref().unwrap_or(&inferred.native);

    // The effective type currently declared (prefers the @return tag,
    // falls back to the native hint).
    let current_effective = current.docblock.as_ref().or(current.native.as_ref());
    let Some(current_effective) = current_effective else {
        return true;
    };
    !our_effective.equivalent(current_effective)
}

/// Build edits using pre-split native and effective types from our
/// own inference (where native is already a valid PHP type hint).
///
/// This is the counterpart of [`build_update_return_type_edits`] for
/// when we trust our own inference rather than the PHPStan tip.  The
/// difference is that the caller provides the native/effective split
/// directly (e.g. native=`array`, effective=`list<int>`) instead of
/// a single type string that gets split via `PhpType::to_native_hint`.
fn build_update_return_type_edits_split(
    content: &str,
    diag_line: usize,
    native_type: &PhpType,
    effective_type: Option<&PhpType>,
) -> Option<Vec<TextEdit>> {
    // The effective type is the full type for the @return tag.
    // If there is no effective type, the native type is used for both
    // and no docblock is needed.
    let native_str = native_type.to_string();
    let full_type = effective_type
        .map(|e| e.to_string())
        .unwrap_or_else(|| native_str.clone());
    let has_docblock_type = effective_type.is_some();

    let lines: Vec<&str> = content.lines().collect();
    if diag_line >= lines.len() {
        return None;
    }

    let mut edits = Vec::new();

    // ── Step 1: Update native type hint ─────────────────────────────
    let brace_line = find_function_open_brace_line(&lines, diag_line)?;
    let (paren_line, paren_col) = find_close_paren_before_brace(&lines, brace_line)?;

    if let Some(type_edit) =
        find_return_type_edit(&lines, paren_line, paren_col, brace_line, &native_str)
    {
        edits.push(type_edit);
    }

    // ── Step 2: Update @return docblock tag ──────────────────────────
    let func_line = find_func_keyword_line(&lines, paren_line)?;
    let docblock_info = find_function_docblock(&lines, func_line);

    if docblock_info.has_docblock && docblock_info.has_return_tag {
        // Replace the existing @return tag's type.
        if let Some(tag_line) = docblock_info.return_tag_line {
            let line_text = lines[tag_line];
            if let Some(at_pos) = line_text.find("@return") {
                let after_return = &line_text[at_pos + "@return".len()..];
                let type_start = after_return
                    .find(|c: char| !c.is_whitespace())
                    .unwrap_or(after_return.len());
                let type_text = &after_return[type_start..];
                let (_, remainder) = split_type_token(type_text);
                let description = remainder.to_string();

                let new_line = format!(
                    "{}@return {}{}",
                    &line_text[..at_pos],
                    full_type,
                    description
                );

                edits.push(TextEdit {
                    range: Range {
                        start: Position::new(tag_line as u32, 0),
                        end: Position::new(tag_line as u32, line_text.len() as u32),
                    },
                    new_text: new_line,
                });
            }
        }
    } else if has_docblock_type {
        // Only create/insert a @return tag when the effective type
        // differs from the native type.
        let indent = &docblock_info.indent;

        if docblock_info.has_docblock {
            let doc_end = docblock_info.doc_end_line;
            let close_line = lines[doc_end];

            if docblock_info.doc_start_line == doc_end {
                let trimmed = close_line.trim();
                let inner = trimmed
                    .strip_prefix("/**")
                    .and_then(|s| s.strip_suffix("*/"))
                    .map(|s| s.trim())
                    .unwrap_or("");

                let mut new_doc = format!("{}/**\n", indent);
                if !inner.is_empty() {
                    new_doc.push_str(&format!("{} * {}\n", indent, inner));
                    new_doc.push_str(&format!("{} *\n", indent));
                }
                new_doc.push_str(&format!("{} * @return {}\n", indent, full_type));
                new_doc.push_str(&format!("{} */", indent));

                edits.push(TextEdit {
                    range: Range {
                        start: Position::new(doc_end as u32, 0),
                        end: Position::new(doc_end as u32, close_line.len() as u32),
                    },
                    new_text: new_doc,
                });
            } else {
                let prev_line = if doc_end > docblock_info.doc_start_line {
                    lines[doc_end - 1].trim()
                } else {
                    ""
                };
                let prev_trimmed = prev_line.trim_start_matches('*').trim();
                let needs_separator = !prev_trimmed.is_empty()
                    && !prev_trimmed.starts_with("@return")
                    && !prev_trimmed.starts_with("@throws")
                    && prev_trimmed.starts_with('@');

                let mut insert_text = String::new();
                if needs_separator {
                    insert_text.push_str(&format!("{} *\n", indent));
                }
                insert_text.push_str(&format!("{} * @return {}\n", indent, full_type));

                edits.push(TextEdit {
                    range: Range {
                        start: Position::new(doc_end as u32, 0),
                        end: Position::new(doc_end as u32, 0),
                    },
                    new_text: insert_text,
                });
            }
        } else {
            let new_doc = format!(
                "{}/**\n{} * @return {}\n{} */\n",
                indent, indent, full_type, indent
            );
            edits.push(TextEdit {
                range: Range {
                    start: Position::new(func_line as u32, 0),
                    end: Position::new(func_line as u32, 0),
                },
                new_text: new_doc,
            });
        }
    }

    if edits.is_empty() {
        return None;
    }

    Some(edits)
}

/// Build a list of `TextEdit`s that update both the native return type
/// hint and the `@return` docblock tag for a `return.type` diagnostic.
///
/// The native type is changed to the base type (generics stripped) so
/// that it remains valid PHP.  The `@return` tag gets the full type
/// including any generic parameters.
///
/// When the actual type has no generics and there is no existing
/// `@return` tag, only the native type is changed.
///
/// Returns `None` if the enclosing function cannot be found.
fn build_update_return_type_edits(
    content: &str,
    diag_line: usize,
    actual_type: &PhpType,
) -> Option<Vec<TextEdit>> {
    let lines: Vec<&str> = content.lines().collect();
    if diag_line >= lines.len() {
        return None;
    }

    let mut edits = Vec::new();

    let actual_str = actual_type.to_string();
    let base_type = actual_type
        .to_native_hint()
        .unwrap_or_else(|| actual_str.clone());
    let has_generics = base_type != actual_str;

    // ── Step 1: Update native type hint ─────────────────────────────
    let brace_line = find_function_open_brace_line(&lines, diag_line)?;
    let (paren_line, paren_col) = find_close_paren_before_brace(&lines, brace_line)?;

    // Only change the native type if the base type differs from the
    // current native type.
    if let Some(type_edit) =
        find_return_type_edit(&lines, paren_line, paren_col, brace_line, &base_type)
    {
        edits.push(type_edit);
    }

    // ── Step 2: Update @return docblock tag ──────────────────────────
    let func_line = find_func_keyword_line(&lines, paren_line)?;
    let docblock_info = find_function_docblock(&lines, func_line);

    if docblock_info.has_docblock && docblock_info.has_return_tag {
        // Replace the existing @return tag's type.
        if let Some(tag_line) = docblock_info.return_tag_line {
            let line_text = lines[tag_line];
            if let Some(at_pos) = line_text.find("@return") {
                let after_return = &line_text[at_pos + "@return".len()..];
                let type_start = after_return
                    .find(|c: char| !c.is_whitespace())
                    .unwrap_or(after_return.len());
                let type_text = &after_return[type_start..];
                let (_, remainder) = split_type_token(type_text);
                let description = remainder.to_string();

                let new_line = format!(
                    "{}@return {}{}",
                    &line_text[..at_pos],
                    actual_str,
                    description
                );

                edits.push(TextEdit {
                    range: Range {
                        start: Position::new(tag_line as u32, 0),
                        end: Position::new(tag_line as u32, line_text.len() as u32),
                    },
                    new_text: new_line,
                });
            }
        }
    } else if has_generics {
        // Only create/insert a @return tag when the actual type has
        // generics — otherwise the native type hint is sufficient.
        let indent = &docblock_info.indent;

        if docblock_info.has_docblock {
            // Docblock exists but has no @return tag — insert one.
            let doc_end = docblock_info.doc_end_line;
            let close_line = lines[doc_end];

            if docblock_info.doc_start_line == doc_end {
                // Single-line docblock: convert to multi-line.
                let trimmed = close_line.trim();
                let inner = trimmed
                    .strip_prefix("/**")
                    .and_then(|s| s.strip_suffix("*/"))
                    .map(|s| s.trim())
                    .unwrap_or("");

                let mut new_doc = format!("{}/**\n", indent);
                if !inner.is_empty() {
                    new_doc.push_str(&format!("{} * {}\n", indent, inner));
                    new_doc.push_str(&format!("{} *\n", indent));
                }
                new_doc.push_str(&format!("{} * @return {}\n", indent, actual_str));
                new_doc.push_str(&format!("{} */", indent));

                edits.push(TextEdit {
                    range: Range {
                        start: Position::new(doc_end as u32, 0),
                        end: Position::new(doc_end as u32, close_line.len() as u32),
                    },
                    new_text: new_doc,
                });
            } else {
                // Multi-line docblock: insert @return before `*/`.
                let prev_line = if doc_end > docblock_info.doc_start_line {
                    lines[doc_end - 1].trim()
                } else {
                    ""
                };
                let prev_trimmed = prev_line.trim_start_matches('*').trim();
                let needs_separator = !prev_trimmed.is_empty()
                    && !prev_trimmed.starts_with("@return")
                    && !prev_trimmed.starts_with("@throws")
                    && prev_trimmed.starts_with('@');

                let mut insert_text = String::new();
                if needs_separator {
                    insert_text.push_str(&format!("{} *\n", indent));
                }
                insert_text.push_str(&format!("{} * @return {}\n", indent, actual_str));

                edits.push(TextEdit {
                    range: Range {
                        start: Position::new(doc_end as u32, 0),
                        end: Position::new(doc_end as u32, 0),
                    },
                    new_text: insert_text,
                });
            }
        } else {
            // No existing docblock — create one with `@return`.
            let new_doc = format!(
                "{}/**\n{} * @return {}\n{} */\n",
                indent, indent, actual_str, indent
            );
            edits.push(TextEdit {
                range: Range {
                    start: Position::new(func_line as u32, 0),
                    end: Position::new(func_line as u32, 0),
                },
                new_text: new_doc,
            });
        }
    }

    if edits.is_empty() {
        return None;
    }

    Some(edits)
}

/// Extract the actual return type from a `return.void` diagnostic
/// message.
///
/// Message format:
/// `{desc} with return type void returns {actual} but should not return anything.`
///
/// Returns the `{actual}` type as a `PhpType`, or `None` if the
/// message doesn't match.
fn extract_actual_type(message: &str) -> Option<PhpType> {
    let marker = " returns ";
    let start = message.find(marker)? + marker.len();
    let rest = &message[start..];
    let end = rest.find(" but should not return anything.")?;
    let actual = rest[..end].trim();
    if actual.is_empty() {
        return None;
    }
    Some(PhpType::parse(actual))
}

/// Extract the actual return type from a `return.type` diagnostic
/// message.
///
/// Message format:
/// `{desc} should return {expected} but returns {actual}.`
///
/// Returns the `{actual}` type as a `PhpType`, or `None` if the
/// message doesn't match.
fn extract_return_type_actual(message: &str) -> Option<PhpType> {
    let marker = " but returns ";
    let start = message.find(marker)? + marker.len();
    let rest = &message[start..];
    // Strip the trailing period.
    let actual = rest.strip_suffix('.')?.trim();
    if actual.is_empty() {
        return None;
    }
    Some(PhpType::parse(actual))
}

/// Infer a PHP type from a literal return expression (cheap, no
/// resolution needed).
///
/// Delegates to the shared `crate::util::infer_type_from_literal()`
/// for basic scalar/null/string/empty-array literals, then handles
/// extended cases (array literals with elements, `new ClassName()`).
///
/// Returns `None` for anything that isn't a simple literal — the
/// caller should fall back to the full type resolver for those.
fn infer_type_from_literal(expr: &str) -> Option<PhpType> {
    // Try the shared utility for basic literals.
    if let Some(t) = crate::util::infer_type_from_literal(expr) {
        return Some(t);
    }

    // Array literal with elements.
    if expr.starts_with('[') && expr.ends_with(']') {
        return infer_array_literal_type(&expr[1..expr.len() - 1]);
    }
    if expr.starts_with("array(") && expr.ends_with(')') {
        return infer_array_literal_type(&expr[6..expr.len() - 1]);
    }

    // `new ClassName(...)` — extract the class name.
    if let Some(rest) = expr.strip_prefix("new ") {
        let class_name = rest
            .split(|c: char| c == '(' || c.is_whitespace())
            .next()
            .unwrap_or("")
            .trim();
        if !class_name.is_empty() {
            return Some(PhpType::Named(class_name.to_string()));
        }
    }

    // Not a literal — caller should use the full resolver.
    None
}

/// Infer a type from the comma-separated contents of an array literal.
///
/// Handles simple cases where all elements are the same scalar type
/// (e.g. `['a', 'b']` → `list<string>`, `[1, 2, 3]` → `list<int>`).
/// Key-value pairs with string keys produce `array<string, V>`.
/// Falls back to `array` when elements are mixed or too complex.
fn infer_array_literal_type(inner: &str) -> Option<PhpType> {
    let inner = inner.trim();
    if inner.is_empty() {
        return Some(PhpType::array());
    }

    // Split on commas at the top level (not inside nested brackets,
    // parens, or strings).
    let elements = split_array_elements(inner);
    if elements.is_empty() {
        return Some(PhpType::array());
    }

    let mut value_types: Vec<PhpType> = Vec::new();
    let mut has_string_keys = false;
    let mut has_int_keys = false;

    for elem in &elements {
        let elem = elem.trim();
        if elem.is_empty() {
            continue;
        }

        // Check for key => value syntax.
        if let Some(arrow_pos) = find_top_level_arrow(elem) {
            let key = elem[..arrow_pos].trim();
            let value = elem[arrow_pos + 2..].trim();

            if (key.starts_with('\'') && key.ends_with('\''))
                || (key.starts_with('"') && key.ends_with('"'))
            {
                has_string_keys = true;
            } else if key.parse::<i64>().is_ok() {
                has_int_keys = true;
            } else {
                // Complex key expression — bail.
                return Some(PhpType::array());
            }

            match infer_type_from_literal(value) {
                Some(t) => value_types.push(t),
                None => return Some(PhpType::array()),
            }
        } else {
            // Sequential element (no key).
            match infer_type_from_literal(elem) {
                Some(t) => value_types.push(t),
                None => return Some(PhpType::array()),
            }
        }
    }

    if value_types.is_empty() {
        return Some(PhpType::array());
    }

    // Deduplicate value types.
    let mut deduped: Vec<PhpType> = Vec::with_capacity(value_types.len());
    for ty in &value_types {
        if !deduped.iter().any(|existing| existing.equivalent(ty)) {
            deduped.push(ty.clone());
        }
    }

    let value_union_type = if deduped.len() > 3 {
        PhpType::mixed()
    } else if deduped.len() == 1 {
        deduped.into_iter().next().unwrap()
    } else {
        PhpType::Union(deduped)
    };

    if has_string_keys && !has_int_keys {
        Some(PhpType::generic_array(PhpType::string(), value_union_type))
    } else if has_string_keys {
        // Mixed key types — just use array with value type.
        Some(PhpType::generic_array_val(value_union_type))
    } else {
        Some(PhpType::list(value_union_type))
    }
}

/// Split array element text on top-level commas (not inside nested
/// brackets, parentheses, or string literals).
fn split_array_elements(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut start = 0;

    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let ch = bytes[i] as char;
        match ch {
            '\'' if !in_double_quote => in_single_quote = !in_single_quote,
            '"' if !in_single_quote => in_double_quote = !in_double_quote,
            '[' | '(' if !in_single_quote && !in_double_quote => depth += 1,
            ']' | ')' if !in_single_quote && !in_double_quote => depth -= 1,
            ',' if depth == 0 && !in_single_quote && !in_double_quote => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            '\\' if in_single_quote || in_double_quote => {
                // Skip escaped character inside strings.
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    if start < s.len() {
        parts.push(&s[start..]);
    }
    parts
}

/// Find the position of `=>` at the top level of an array element
/// (not inside nested brackets, parens, or strings).
fn find_top_level_arrow(s: &str) -> Option<usize> {
    let mut depth = 0i32;
    let mut in_single_quote = false;
    let mut in_double_quote = false;

    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let ch = bytes[i] as char;
        match ch {
            '\'' if !in_double_quote => in_single_quote = !in_single_quote,
            '"' if !in_single_quote => in_double_quote = !in_double_quote,
            '[' | '(' if !in_single_quote && !in_double_quote => depth += 1,
            ']' | ')' if !in_single_quote && !in_double_quote => depth -= 1,
            '=' if depth == 0
                && !in_single_quote
                && !in_double_quote
                && i + 1 < bytes.len()
                && bytes[i + 1] == b'>' =>
            {
                return Some(i);
            }
            '\\' if in_single_quote || in_double_quote => {
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Check whether there is already a return type hint between `)` and
/// `{`.  Returns `true` if a `:` is found in that region.
fn has_return_type_between(
    lines: &[&str],
    paren_line: usize,
    paren_col: usize,
    brace_line: usize,
) -> bool {
    for (line_idx, line) in lines
        .iter()
        .enumerate()
        .take(brace_line + 1)
        .skip(paren_line)
    {
        let start_col = if line_idx == paren_line {
            paren_col + 1
        } else {
            0
        };
        let end_col = if line_idx == brace_line {
            line.find('{').unwrap_or(line.len())
        } else {
            line.len()
        };
        if start_col <= end_col && line[start_col..end_col].contains(':') {
            return true;
        }
    }
    false
}

/// Check whether the byte position `after_semi` (just past a `;`) is
/// the last statement in its enclosing function body.
///
/// Scans forward from `after_semi` through whitespace, comments, and
/// closing braces.  If only `}` characters (closing nested blocks like
/// `if`/`foreach`/`try`) and whitespace/comments appear between the
/// `;` and the function's own closing `}`, then the statement is the
/// last one in the function and a trailing `return;` would be
/// redundant.
///
/// Returns `false` when any other statement or token appears, meaning
/// the `return;` is needed to exit early.
fn is_last_statement_in_function(content: &str, after_semi: usize) -> bool {
    let bytes = content.as_bytes();
    let mut i = after_semi;

    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\t' | b'\r' | b'\n' => {
                i += 1;
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                // Line comment — skip to end of line.
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                // Block comment — skip to `*/`.
                i += 2;
                while i + 1 < bytes.len() {
                    if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                        i += 2;
                        break;
                    }
                    i += 1;
                }
            }
            b'}' => {
                // Closing brace — could be an `if`/`foreach`/etc.
                // block or the function itself.  Keep scanning to
                // see if anything other than more `}` and whitespace
                // follows.
                i += 1;
            }
            _ => return false,
        }
    }
    // Reached end of content with only `}` and whitespace — the
    // statement was the last one.
    true
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Walk backward from `start_line` to find the line containing the
/// opening `{` of the enclosing function body.
///
/// The opening brace is the first `{` found scanning backward that is
/// not inside a string or comment.  We use a simple heuristic: look
/// for a line whose trimmed content ends with `{` or contains `{`
/// after a `)`.
///
/// **Limitation:** Braces inside string literals and comments are
/// counted, which can produce wrong results in rare cases.  A fully
/// correct backward scan would require re-parsing from the top of the
/// file.  This simple heuristic works for typical PHP code.
fn find_function_open_brace_line(lines: &[&str], start_line: usize) -> Option<usize> {
    // Track brace depth: we start inside the function body (depth 1)
    // and look backward for the opening `{`.
    let mut depth: i32 = 0;
    for i in (0..start_line).rev() {
        let line = lines[i];
        // Count braces on this line (simple heuristic, ignoring strings).
        for ch in line.chars() {
            match ch {
                '{' => depth -= 1,
                '}' => depth += 1,
                _ => {}
            }
        }
        if depth < 0 {
            return Some(i);
        }
    }
    None
}

/// Search forward from a declaration line to find the opening `{` of
/// the function body.
///
/// Checks the declaration line itself and up to 5 lines after it.
/// Returns the line number containing `{`, or `None`.
fn find_open_brace_from_declaration(lines: &[&str], decl_line: usize) -> Option<usize> {
    let end = (decl_line + 6).min(lines.len());
    (decl_line..end).find(|&i| lines[i].contains('{'))
}

/// Find the closing `)` of the parameter list before the opening `{`.
///
/// Scans backward from `brace_line` looking for `)`.
fn find_close_paren_before_brace(lines: &[&str], brace_line: usize) -> Option<(usize, usize)> {
    // First check the brace line itself (before the `{`).
    let brace_text = lines[brace_line];
    if let Some(brace_pos) = brace_text.rfind('{') {
        let before_brace = &brace_text[..brace_pos];
        if let Some(paren_pos) = before_brace.rfind(')') {
            return Some((brace_line, paren_pos));
        }
    }

    // Walk backward to find `)`.
    for i in (0..brace_line).rev() {
        if let Some(paren_pos) = lines[i].rfind(')') {
            return Some((i, paren_pos));
        }
    }

    None
}

/// Gather the source text between the closing `)` at `(paren_line, paren_col)`
/// and the opening `{` on `brace_line`.
///
/// The result spans from column `paren_col + 1` on `paren_line` to just
/// before the `{` on `brace_line`, with newlines between lines.
fn gather_between_paren_and_brace(
    lines: &[&str],
    paren_line: usize,
    paren_col: usize,
    brace_line: usize,
) -> String {
    let mut between = String::new();

    for (line_idx, line) in lines
        .iter()
        .enumerate()
        .take(brace_line + 1)
        .skip(paren_line)
    {
        let start_col = if line_idx == paren_line {
            paren_col + 1
        } else {
            0
        };
        let end_col = if line_idx == brace_line {
            line.find('{').unwrap_or(line.len())
        } else {
            line.len()
        };
        if start_col <= end_col {
            between.push_str(&line[start_col..end_col]);
        }
        if line_idx < brace_line {
            between.push('\n');
        }
    }

    between
}

/// Find the return type hint between the closing `)` and opening `{`,
/// and build a `TextEdit` that replaces it with `: {target_type}`.
///
/// Looks for the pattern `: TypeName` (with optional whitespace and
/// nullable `?` prefix).  Returns `None` if the current type already
/// matches `target_type`.
fn find_return_type_edit(
    lines: &[&str],
    paren_line: usize,
    paren_col: usize,
    brace_line: usize,
    target_type: &str,
) -> Option<TextEdit> {
    // Gather the text between `)` and `{` across potentially multiple
    // lines.
    let between = gather_between_paren_and_brace(lines, paren_line, paren_col, brace_line);

    // Find `: Type` in the between text.
    let colon_pos = between.find(':')?;
    let after_colon = &between[colon_pos + 1..];
    let type_start_offset = after_colon.find(|c: char| !c.is_whitespace()).unwrap_or(0);
    let type_text_start = colon_pos + 1 + type_start_offset;
    let type_text = &between[type_text_start..];

    // The type name ends at the first whitespace, `{`, or end of
    // the between text.
    let type_len = type_text
        .find(|c: char| c.is_whitespace() || c == '{')
        .unwrap_or(type_text.len());

    if type_len == 0 {
        return None;
    }

    let type_name = &type_text[..type_len];
    if type_name == target_type {
        return None;
    }

    // Convert the offset within `between` to a line/col position.
    // The colon_pos tells us where `:` is; the type starts at
    // `type_text_start` and ends at `type_text_start + type_len`.

    // Map `colon_pos` back to an absolute line/col.
    let colon_abs = map_between_offset_to_position(lines, paren_line, paren_col, colon_pos)?;
    let type_end_abs =
        map_between_offset_to_position(lines, paren_line, paren_col, type_text_start + type_len)?;

    Some(TextEdit {
        range: Range {
            start: Position::new(colon_abs.0 as u32, colon_abs.1 as u32),
            end: Position::new(type_end_abs.0 as u32, type_end_abs.1 as u32),
        },
        new_text: format!(": {}", target_type),
    })
}

/// Map an offset within the "between" text back to an absolute
/// (line, col) position in the original source.
fn map_between_offset_to_position(
    lines: &[&str],
    paren_line: usize,
    paren_col: usize,
    offset: usize,
) -> Option<(usize, usize)> {
    // Re-walk the between region character by character.
    let mut remaining = offset;
    for (line_idx, line) in lines.iter().enumerate().skip(paren_line) {
        let start_col = if line_idx == paren_line {
            paren_col + 1
        } else {
            0
        };
        let end_col = line.len();
        let span = end_col - start_col;

        if remaining <= span {
            return Some((line_idx, start_col + remaining));
        }
        remaining -= span;

        // Account for the newline character.
        if remaining == 0 {
            // Exactly at the newline boundary — start of next line.
            return Some((line_idx + 1, 0));
        }
        remaining -= 1; // for the '\n'
    }
    None
}

/// Look for a docblock above the function signature and remove any
/// `@return` tag line from it.
fn find_and_remove_return_tag(lines: &[&str], func_line: usize) -> Option<TextEdit> {
    if func_line == 0 {
        return None;
    }

    // Walk backward from the line before the function to find the
    // docblock.  Skip attribute lines like `#[Override]`.
    let mut doc_end_line = None;
    for i in (0..func_line).rev() {
        let trimmed = lines[i].trim();
        if trimmed.ends_with("*/") {
            doc_end_line = Some(i);
            break;
        }
        // Skip attributes and blank lines between function and docblock.
        if trimmed.starts_with("#[") || trimmed.is_empty() {
            continue;
        }
        // Hit non-docblock, non-attribute content — no docblock.
        break;
    }

    let doc_end_line = doc_end_line?;

    // Find the start of the docblock.
    let mut doc_start_line = doc_end_line;
    for i in (0..=doc_end_line).rev() {
        let trimmed = lines[i].trim();
        if trimmed.starts_with("/**") {
            doc_start_line = i;
            break;
        }
        if trimmed.starts_with('*') || trimmed.starts_with("/*") {
            continue;
        }
        break;
    }

    // Look for a `@return` line within the docblock.
    let return_line =
        (doc_start_line..=doc_end_line).find(|&i| lines[i].trim().contains("@return"))?;

    Some(TextEdit {
        range: Range {
            start: Position::new(return_line as u32, 0),
            end: Position::new((return_line + 1) as u32, 0),
        },
        new_text: String::new(),
    })
}

// ── Stale detection ─────────────────────────────────────────────────────────

/// Check whether a `return.void` or `return.empty` diagnostic is stale.
///
/// For `return.void`: the diagnostic is stale when the diagnostic line
/// contains `return;` (bare return, no expression) — meaning the
/// expression has already been stripped.
///
/// For `return.empty`: the diagnostic is stale when the enclosing
/// function's return type declaration already says `void`.
///
/// Called from `is_stale_phpstan_diagnostic` in `diagnostics/mod.rs`.
pub(crate) fn is_fix_return_type_stale(content: &str, diag_line: usize, identifier: &str) -> bool {
    let lines: Vec<&str> = content.lines().collect();

    if diag_line >= lines.len() {
        return true; // line doesn't exist any more → stale
    }

    match identifier {
        RETURN_VOID_ID => {
            // Stale if the line no longer contains a return with an
            // expression (user either stripped it or changed the type).
            let trimmed = lines[diag_line].trim();
            !trimmed.contains("return ") || trimmed == "return;"
        }
        RETURN_TYPE_ID => {
            // No content heuristic — the fix might be to change the
            // code rather than the type, so we can't tell from the
            // source alone.  Cleared eagerly by codeAction/resolve.
            false
        }
        MISSING_TYPE_RETURN_ID => {
            // The diagnostic is reported on the function declaration
            // line itself.  Stale if `)` is followed by `:` (a return
            // type has been added).  Simple text check on the line.
            let line = lines[diag_line];
            if let Some(paren_pos) = line.rfind(')') {
                line[paren_pos + 1..].contains(':')
            } else {
                false
            }
        }
        RETURN_EMPTY_ID => {
            // Stale if the enclosing function's return type is already
            // `void`.  The diagnostic is on a `return;` inside the
            // body, so search backward for the opening brace.
            let brace_line = match find_function_open_brace_line(&lines, diag_line) {
                Some(l) => l,
                None => return false,
            };
            let (paren_line, paren_col) = match find_close_paren_before_brace(&lines, brace_line) {
                Some(p) => p,
                None => return false,
            };

            // Gather text between `)` and `{` and check if the return
            // type is already `void`.
            let between = gather_between_paren_and_brace(&lines, paren_line, paren_col, brace_line);

            // Look for `: void` in the between text.
            if let Some(colon_pos) = between.find(':') {
                let after_colon = between[colon_pos + 1..].trim();
                // The type name after `:` ends at whitespace or `{`.
                let type_word = after_colon
                    .split(|c: char| c.is_whitespace() || c == '{')
                    .next()
                    .unwrap_or("");
                PhpType::parse(type_word).is_void()
            } else {
                false
            }
        }
        _ => false,
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── build_strip_return_expr_edit ────────────────────────────────

    #[test]
    fn removes_return_keeps_expression_omits_redundant_return() {
        // Last statement in function — no need for bare `return;`.
        let content = "<?php\nfunction foo(): void {\n    return 42;\n}\n";
        let edit = build_strip_return_expr_edit(content, 2).unwrap();
        assert_eq!(edit.range.start, Position::new(2, 4));
        assert_eq!(edit.range.end, Position::new(2, 14));
        assert_eq!(edit.new_text, "42;");
    }

    #[test]
    fn removes_return_string() {
        // Last statement in function — no bare `return;`.
        let content = "<?php\nfunction foo(): void {\n    return 'hello';\n}\n";
        let edit = build_strip_return_expr_edit(content, 2).unwrap();
        assert_eq!(edit.new_text, "'hello';");
    }

    #[test]
    fn removes_return_method_call() {
        // Last statement in method — no bare `return;`.
        let content = "<?php\nclass A {\n    public function run(): void {\n        return $this->doWork();\n    }\n}\n";
        let edit = build_strip_return_expr_edit(content, 3).unwrap();
        assert_eq!(edit.new_text, "$this->doWork();");
    }

    #[test]
    fn removes_return_in_if_block_with_more_code() {
        // NOT the last statement — there's `echo 'more';` after the if block.
        let content = "<?php\nclass A {\n    public function run(): void {\n        if (true) {\n            return $this->doWork();\n        }\n        echo 'more';\n    }\n}\n";
        let edit = build_strip_return_expr_edit(content, 4).unwrap();
        assert_eq!(edit.new_text, "$this->doWork();\n            return;");
    }

    #[test]
    fn return_null_becomes_bare_return() {
        // `return null;` → `return;` (null is not meaningful in void)
        let content = "<?php\nfunction foo(): void {\n    return null;\n}\n";
        let edit = build_strip_return_expr_edit(content, 2).unwrap();
        assert_eq!(edit.new_text, "return;");
    }

    #[test]
    fn strips_return_expression_variable() {
        // Last statement — no bare `return;`.
        let content = "<?php\nfunction foo(): void {\n    return $value;\n}\n";
        let edit = build_strip_return_expr_edit(content, 2).unwrap();
        assert_eq!(edit.new_text, "$value;");
        assert_eq!(edit.range.start, Position::new(2, 4));
    }

    #[test]
    fn strips_multiline_return_expression() {
        // Last statement — no bare `return;`.
        let content =
            "<?php\nfunction foo(): void {\n    return array(\n        1,\n        2\n    );\n}\n";
        let edit = build_strip_return_expr_edit(content, 2).unwrap();
        assert_eq!(edit.new_text, "array(\n        1,\n        2\n    );");
        assert_eq!(edit.range.start, Position::new(2, 4));
        // The `;` is on line 5 (0-indexed)
        assert_eq!(edit.range.end.line, 5);
    }

    #[test]
    fn strips_return_in_if_block_last_statement() {
        // return is inside an if block, but it IS the last statement
        // in the function (only `}` closers follow).
        let content = "<?php\nclass A {\n    public function run(): void {\n        if (true) {\n            return $this->doWork();\n        }\n    }\n}\n";
        let edit = build_strip_return_expr_edit(content, 4).unwrap();
        assert_eq!(edit.new_text, "$this->doWork();");
    }

    #[test]
    fn returns_none_when_already_bare_return() {
        let content = "<?php\nfunction foo(): void {\n    return;\n}\n";
        assert!(build_strip_return_expr_edit(content, 2).is_none());
    }

    #[test]
    fn returns_none_for_invalid_line() {
        let content = "<?php\n";
        assert!(build_strip_return_expr_edit(content, 5).is_none());
    }

    #[test]
    fn returns_none_when_no_return_on_line() {
        let content = "<?php\nfunction foo(): void {\n    $x = 1;\n}\n";
        assert!(build_strip_return_expr_edit(content, 2).is_none());
    }

    // ── build_change_return_type_edits_to ───────────────────────────

    #[test]
    fn changes_return_type_to_void() {
        let content = "<?php\nfunction foo(): int {\n    return;\n}\n";
        let edits = build_change_return_type_edits_to(content, 2, &PhpType::void()).unwrap();
        assert_eq!(edits.len(), 1);
        let edit = &edits[0];
        assert_eq!(edit.new_text, ": void");
        // Verify it replaces `: int`
        let lines: Vec<&str> = content.lines().collect();
        let replaced = &lines[edit.range.start.line as usize]
            [edit.range.start.character as usize..edit.range.end.character as usize];
        assert_eq!(replaced, ": int");
    }

    #[test]
    fn changes_return_type_string() {
        let content = "<?php\nfunction foo(): string {\n    return;\n}\n";
        let edits = build_change_return_type_edits_to(content, 2, &PhpType::void()).unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, ": void");
    }

    #[test]
    fn changes_return_type_to_actual() {
        let content = "<?php\nfunction foo(): void {\n    return 42;\n}\n";
        let edits = build_change_return_type_edits_to(content, 2, &PhpType::parse("int")).unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, ": int");
    }

    #[test]
    fn changes_void_to_string() {
        let content = "<?php\nfunction foo(): void {\n    return 'hello';\n}\n";
        let edits =
            build_change_return_type_edits_to(content, 2, &PhpType::parse("string")).unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, ": string");
    }

    #[test]
    fn changes_nullable_return_type() {
        let content = "<?php\nfunction foo(): ?string {\n    return;\n}\n";
        let edits = build_change_return_type_edits_to(content, 2, &PhpType::void()).unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, ": void");
    }

    #[test]
    fn changes_return_type_and_removes_return_tag() {
        let content =
            "<?php\n/**\n * @return int The value\n */\nfunction foo(): int {\n    return;\n}\n";
        let edits = build_change_return_type_edits_to(content, 5, &PhpType::void()).unwrap();
        assert_eq!(edits.len(), 2);

        // One edit replaces the type, one removes the @return line.
        let type_edit = edits.iter().find(|e| e.new_text == ": void").unwrap();
        let tag_edit = edits.iter().find(|e| e.new_text.is_empty()).unwrap();

        // The type edit should be on the function line (line 4).
        assert_eq!(type_edit.range.start.line, 4);

        // The @return tag is on line 2.
        assert_eq!(tag_edit.range.start.line, 2);
        assert_eq!(tag_edit.range.end.line, 3);
    }

    #[test]
    fn does_not_change_when_already_void() {
        let content = "<?php\nfunction foo(): void {\n    return;\n}\n";
        assert!(build_change_return_type_edits_to(content, 2, &PhpType::void()).is_none());
    }

    #[test]
    fn does_not_change_when_already_matches_actual() {
        let content = "<?php\nfunction foo(): int {\n    return 42;\n}\n";
        assert!(build_change_return_type_edits_to(content, 2, &PhpType::parse("int")).is_none());
    }

    #[test]
    fn returns_none_when_no_function_found() {
        let content = "<?php\nreturn;\n";
        assert!(build_change_return_type_edits_to(content, 1, &PhpType::void()).is_none());
    }

    #[test]
    fn changes_method_return_type() {
        let content =
            "<?php\nclass Foo {\n    public function bar(): string {\n        return;\n    }\n}\n";
        let edits = build_change_return_type_edits_to(content, 3, &PhpType::void()).unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, ": void");
    }

    // ── is_fix_return_type_stale ───────────────────────────────────

    #[test]
    fn stale_when_return_has_no_expression() {
        let content = "<?php\nfunction foo(): void {\n    return;\n}\n";
        assert!(is_fix_return_type_stale(content, 2, "return.void"));
    }

    #[test]
    fn not_stale_when_return_has_expression() {
        let content = "<?php\nfunction foo(): void {\n    return 42;\n}\n";
        assert!(!is_fix_return_type_stale(content, 2, "return.void"));
    }

    #[test]
    fn stale_return_empty_when_type_is_void() {
        let content = "<?php\nfunction foo(): void {\n    return;\n}\n";
        assert!(is_fix_return_type_stale(content, 2, "return.empty"));
    }

    #[test]
    fn not_stale_return_empty_when_type_is_not_void() {
        let content = "<?php\nfunction foo(): int {\n    return;\n}\n";
        assert!(!is_fix_return_type_stale(content, 2, "return.empty"));
    }

    #[test]
    fn stale_when_line_gone() {
        let content = "<?php\n";
        assert!(is_fix_return_type_stale(content, 5, "return.void"));
        assert!(is_fix_return_type_stale(content, 5, "return.empty"));
    }

    #[test]
    fn not_stale_for_unknown_identifier() {
        let content = "<?php\nfunction foo(): void {\n    return;\n}\n";
        assert!(!is_fix_return_type_stale(content, 2, "other.id"));
    }

    // ── Message matching ───────────────────────────────────────────

    #[test]
    fn matches_return_void_message() {
        let msg =
            "Method Foo::bar() with return type void returns int but should not return anything.";
        assert!(msg.ends_with(RETURN_VOID_MSG_SUFFIX));
    }

    // ── extract_actual_type ─────────────────────────────────────────

    #[test]
    fn extracts_actual_type_int() {
        let msg =
            "Method Foo::bar() with return type void returns int but should not return anything.";
        assert_eq!(extract_actual_type(msg), Some(PhpType::parse("int")));
    }

    #[test]
    fn extracts_actual_type_string() {
        let msg =
            "Function foo() with return type void returns string but should not return anything.";
        assert_eq!(extract_actual_type(msg), Some(PhpType::parse("string")));
    }

    #[test]
    fn extracts_actual_type_union() {
        let msg = "Method X::y() with return type void returns int|string but should not return anything.";
        assert_eq!(extract_actual_type(msg), Some(PhpType::parse("int|string")));
    }

    #[test]
    fn extracts_actual_type_null() {
        let msg =
            "Method X::y() with return type void returns null but should not return anything.";
        assert_eq!(extract_actual_type(msg), Some(PhpType::parse("null")));
    }

    #[test]
    fn extract_actual_type_returns_none_for_unrelated_message() {
        let msg = "Some other message.";
        assert_eq!(extract_actual_type(msg), None);
    }

    // ── extract_return_type_actual (return.type) ────────────────────

    #[test]
    fn extracts_return_type_actual_int() {
        let msg = "Method Foo::bar() should return string but returns int.";
        assert_eq!(extract_return_type_actual(msg), Some(PhpType::parse("int")));
    }

    #[test]
    fn extracts_return_type_actual_union() {
        let msg = "Function foo() should return int but returns int|string.";
        assert_eq!(
            extract_return_type_actual(msg),
            Some(PhpType::parse("int|string"))
        );
    }

    #[test]
    fn extracts_return_type_actual_class() {
        let msg = "Method X::y() should return self but returns App\\Models\\User.";
        assert_eq!(
            extract_return_type_actual(msg),
            Some(PhpType::parse("App\\Models\\User"))
        );
    }

    #[test]
    fn extract_return_type_actual_returns_none_for_unrelated() {
        let msg = "Some other message.";
        assert_eq!(extract_return_type_actual(msg), None);
    }

    // ── has_return_type_between ─────────────────────────────────────

    #[test]
    fn detects_existing_return_type() {
        let lines = vec!["function foo(): int {"];
        // paren at col 13 (the ')'), brace_line = 0
        assert!(has_return_type_between(&lines, 0, 13, 0));
    }

    #[test]
    fn detects_no_return_type() {
        let lines = vec!["function foo() {"];
        // paren at col 13, brace_line = 0
        assert!(!has_return_type_between(&lines, 0, 13, 0));
    }

    // ── infer_type_from_literal ─────────────────────────────────────

    #[test]
    fn literal_int() {
        assert_eq!(
            infer_type_from_literal("42").map(|t| t.to_string()),
            Some("int".to_string())
        );
        assert_eq!(
            infer_type_from_literal("-1").map(|t| t.to_string()),
            Some("int".to_string())
        );
    }

    #[test]
    fn literal_float() {
        assert_eq!(
            infer_type_from_literal("1.5").map(|t| t.to_string()),
            Some("float".to_string())
        );
    }

    #[test]
    fn literal_bool() {
        assert_eq!(
            infer_type_from_literal("true").map(|t| t.to_string()),
            Some("bool".to_string())
        );
        assert_eq!(
            infer_type_from_literal("false").map(|t| t.to_string()),
            Some("bool".to_string())
        );
    }

    #[test]
    fn literal_string() {
        assert_eq!(
            infer_type_from_literal("'hello'").map(|t| t.to_string()),
            Some("string".to_string())
        );
        assert_eq!(
            infer_type_from_literal("\"world\"").map(|t| t.to_string()),
            Some("string".to_string())
        );
    }

    #[test]
    fn literal_array_empty() {
        assert_eq!(
            infer_type_from_literal("[]").map(|t| t.to_string()),
            Some("array".to_string())
        );
    }

    #[test]
    fn literal_array_of_strings() {
        assert_eq!(
            infer_type_from_literal("['string']").map(|t| t.to_string()),
            Some("list<string>".to_string())
        );
        assert_eq!(
            infer_type_from_literal("['a', 'b', 'c']").map(|t| t.to_string()),
            Some("list<string>".to_string())
        );
    }

    #[test]
    fn literal_array_of_ints() {
        assert_eq!(
            infer_type_from_literal("[1, 2, 3]").map(|t| t.to_string()),
            Some("list<int>".to_string())
        );
    }

    #[test]
    fn literal_array_mixed_scalars() {
        assert_eq!(
            infer_type_from_literal("['a', 1]").map(|t| t.to_string()),
            Some("list<string|int>".to_string())
        );
    }

    #[test]
    fn literal_array_with_string_keys() {
        assert_eq!(
            infer_type_from_literal("['key' => 'value']").map(|t| t.to_string()),
            Some("array<string, string>".to_string())
        );
        assert_eq!(
            infer_type_from_literal("['name' => 'Alice', 'age' => 42]").map(|t| t.to_string()),
            Some("array<string, string|int>".to_string())
        );
    }

    #[test]
    fn literal_array_nested() {
        assert_eq!(
            infer_type_from_literal("[['a'], ['b']]").map(|t| t.to_string()),
            Some("list<list<string>>".to_string())
        );
    }

    #[test]
    fn literal_array_with_variable_falls_back() {
        assert_eq!(
            infer_type_from_literal("[$var, 'a']").map(|t| t.to_string()),
            Some("array".to_string())
        );
    }

    #[test]
    fn literal_array_legacy_syntax() {
        assert_eq!(
            infer_type_from_literal("array('a', 'b')").map(|t| t.to_string()),
            Some("list<string>".to_string())
        );
    }

    #[test]
    fn literal_array_new_objects() {
        assert_eq!(
            infer_type_from_literal("[new Foo(), new Foo()]").map(|t| t.to_string()),
            Some("list<Foo>".to_string())
        );
    }

    #[test]
    fn literal_array_trailing_comma() {
        assert_eq!(
            infer_type_from_literal("['a', 'b',]").map(|t| t.to_string()),
            Some("list<string>".to_string())
        );
    }

    #[test]
    fn literal_new_class() {
        assert_eq!(
            infer_type_from_literal("new Foo()").map(|t| t.to_string()),
            Some("Foo".to_string())
        );
    }

    #[test]
    fn literal_null() {
        assert_eq!(
            infer_type_from_literal("null").map(|t| t.to_string()),
            Some("null".to_string())
        );
    }

    #[test]
    fn non_literal_returns_none() {
        assert_eq!(infer_type_from_literal("$var"), None);
        assert_eq!(infer_type_from_literal("$this->bar()"), None);
        assert_eq!(infer_type_from_literal("foo()"), None);
        assert_eq!(infer_type_from_literal("Str::toUpper($x)"), None);
    }

    // ── stale detection for new identifiers ─────────────────────────

    #[test]
    fn return_type_never_stale_via_heuristic() {
        // return.type is only cleared by codeAction/resolve, not by
        // content heuristics, because the right fix might be to change
        // the code rather than the type.
        let content = "<?php\nfunction foo(): int {\n    $x = 1;\n}\n";
        assert!(!is_fix_return_type_stale(content, 2, "return.type"));

        let content2 = "<?php\nfunction foo(): int {\n    return 'hello';\n}\n";
        assert!(!is_fix_return_type_stale(content2, 2, "return.type"));
    }

    #[test]
    fn stale_missing_type_when_type_added() {
        let content = "<?php\nfunction foo(): int {\n    return 1;\n}\n";
        // missingType.return is reported on the function declaration line
        assert!(is_fix_return_type_stale(content, 1, "missingType.return"));
    }

    #[test]
    fn stale_missing_type_multiline_signature() {
        let content = "<?php\nfunction foo(\n    int $x\n): int {\n    return $x;\n}\n";
        // The diagnostic is on the `function` line (line 1), but the
        // `)` and `: int` are on line 3.  PHPStan reports on the
        // function keyword line.  Our simple check looks at the diag
        // line for `)...:`  which won't find it on line 1.  That's
        // acceptable — the diagnostic will be cleared by the next
        // PHPStan run instead of eagerly.
        assert!(!is_fix_return_type_stale(content, 1, "missingType.return"));
    }

    #[test]
    fn not_stale_missing_type_when_no_type() {
        let content = "<?php\nfunction foo() {\n    return 1;\n}\n";
        assert!(!is_fix_return_type_stale(content, 1, "missingType.return"));
    }

    #[test]
    fn matches_return_empty_message() {
        let msg = "Method App\\Foo::bar() should return int but empty return statement found.";
        assert!(msg.contains(RETURN_EMPTY_MSG_FRAGMENT));
    }

    #[test]
    fn rejects_unrelated_message() {
        let msg = "Call to function assert() with true will always evaluate to true.";
        assert!(!msg.ends_with(RETURN_VOID_MSG_SUFFIX));
        assert!(!msg.contains(RETURN_EMPTY_MSG_FRAGMENT));
    }

    // ── Helper tests ───────────────────────────────────────────────

    // ── Docblock @return removal ───────────────────────────────────

    #[test]
    fn change_to_actual_does_not_remove_return_tag() {
        let content = "<?php\n/**\n * @return int The value\n */\nfunction foo(): void {\n    return 42;\n}\n";
        let edits = build_change_return_type_edits_to(content, 5, &PhpType::parse("int")).unwrap();
        // Should only change the type hint, NOT remove the @return tag
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, ": int");
    }

    // ── add return type (missingType.return) ────────────────────────

    #[test]
    fn add_return_type_inserts_after_close_paren_helper() {
        let content = "<?php\nfunction foo() {\n    return 1;\n}\n";
        let lines: Vec<&str> = content.lines().collect();
        let brace_line = find_function_open_brace_line(&lines, 2).unwrap();
        let (paren_line, paren_col) = find_close_paren_before_brace(&lines, brace_line).unwrap();
        assert!(!has_return_type_between(
            &lines, paren_line, paren_col, brace_line
        ));
        assert_eq!(paren_line, 1);
        assert_eq!(paren_col, 13);
    }

    #[test]
    fn removes_return_tag_from_multiline_docblock() {
        let content = "<?php\n/**\n * Does something.\n * @return int\n */\nfunction foo(): int {\n    return;\n}\n";
        let edits = build_change_return_type_edits_to(content, 6, &PhpType::void()).unwrap();
        assert_eq!(edits.len(), 2);
        let tag_edit = edits.iter().find(|e| e.new_text.is_empty()).unwrap();
        assert_eq!(tag_edit.range.start.line, 3);
        assert_eq!(tag_edit.range.end.line, 4);
    }

    #[test]
    fn no_return_tag_edit_when_no_docblock() {
        let content = "<?php\nfunction foo(): int {\n    return;\n}\n";
        let edits = build_change_return_type_edits_to(content, 2, &PhpType::void()).unwrap();
        assert_eq!(edits.len(), 1); // Only the type edit, no tag edit.
    }

    #[test]
    fn no_return_tag_edit_when_docblock_has_no_return() {
        let content =
            "<?php\n/**\n * Does something.\n */\nfunction foo(): int {\n    return;\n}\n";
        let edits = build_change_return_type_edits_to(content, 5, &PhpType::void()).unwrap();
        assert_eq!(edits.len(), 1); // Only the type edit, no tag edit.
    }

    // ── Integration: apply strip edit ──────────────────────────────

    #[test]
    fn apply_strip_edit_produces_correct_content() {
        // `return 42;` is the last statement → replaced with just `42;`
        // (no redundant `return;` since it's the last statement).
        let content = "<?php\nfunction foo(): void {\n    return 42;\n}\n";
        let edit = build_strip_return_expr_edit(content, 2).unwrap();

        // Apply the edit manually.
        let lines: Vec<&str> = content.lines().collect();
        let mut result = String::new();
        for (i, line) in lines.iter().enumerate() {
            if i > 0 {
                result.push('\n');
            }
            if i == edit.range.start.line as usize {
                let prefix = &line[..edit.range.start.character as usize];
                let suffix = if edit.range.end.line as usize == i {
                    &line[edit.range.end.character as usize..]
                } else {
                    ""
                };
                result.push_str(prefix);
                result.push_str(&edit.new_text);
                result.push_str(suffix);
            } else {
                result.push_str(line);
            }
        }
        result.push('\n');

        assert_eq!(result, "<?php\nfunction foo(): void {\n    42;\n}\n");
    }

    #[test]
    fn apply_strip_edit_null_produces_bare_return() {
        let content = "<?php\nfunction foo(): void {\n    return null;\n}\n";
        let edit = build_strip_return_expr_edit(content, 2).unwrap();

        let lines: Vec<&str> = content.lines().collect();
        let mut result = String::new();
        for (i, line) in lines.iter().enumerate() {
            if i > 0 {
                result.push('\n');
            }
            if i == edit.range.start.line as usize {
                let prefix = &line[..edit.range.start.character as usize];
                let suffix = if edit.range.end.line as usize == i {
                    &line[edit.range.end.character as usize..]
                } else {
                    ""
                };
                result.push_str(prefix);
                result.push_str(&edit.new_text);
                result.push_str(suffix);
            } else {
                result.push_str(line);
            }
        }
        result.push('\n');

        assert_eq!(result, "<?php\nfunction foo(): void {\n    return;\n}\n");
    }

    // ── Stale detection after strip fix ─────────────────────────────

    #[test]
    fn stale_after_strip_fix() {
        // Before fix: not stale.
        let before = "<?php\nfunction foo(): void {\n    return 42;\n}\n";
        assert!(!is_fix_return_type_stale(before, 2, "return.void"));

        // After fix (expression kept, no redundant return;): stale
        // because the line no longer has `return ` (it now has `42;`).
        let after = "<?php\nfunction foo(): void {\n    42;\n}\n";
        assert!(is_fix_return_type_stale(after, 2, "return.void"));
    }

    // ── PhpType::to_native_hint (replaces strip_generic_params) ────

    #[test]
    fn strip_generic_simple_type() {
        let parsed = PhpType::parse("int");
        assert_eq!(
            parsed.to_native_hint().unwrap_or_else(|| "int".to_string()),
            "int"
        );
    }

    #[test]
    fn strip_generic_array_with_params() {
        let parsed = PhpType::parse("array<int, string>");
        assert_eq!(
            parsed
                .to_native_hint()
                .unwrap_or_else(|| "array<int, string>".to_string()),
            "array"
        );
    }

    #[test]
    fn strip_generic_nested() {
        let parsed = PhpType::parse("array<int, array<string, bool>>");
        assert_eq!(
            parsed
                .to_native_hint()
                .unwrap_or_else(|| "array<int, array<string, bool>>".to_string()),
            "array"
        );
    }

    #[test]
    fn strip_generic_union_no_generics() {
        let parsed = PhpType::parse("int|string");
        assert_eq!(
            parsed
                .to_native_hint()
                .unwrap_or_else(|| "int|string".to_string()),
            "int|string"
        );
    }

    // ── split_type_token (replaces find_phpdoc_type_end) ───────────

    #[test]
    fn phpdoc_type_end_simple() {
        let (tok, _) = split_type_token("int The value");
        assert_eq!(tok, "int");
    }

    #[test]
    fn phpdoc_type_end_generic() {
        let (tok, _) = split_type_token("array<int, string> The value");
        assert_eq!(tok, "array<int, string>");
    }

    #[test]
    fn phpdoc_type_end_nested_generic() {
        let (tok, _) = split_type_token("array<int, array<string, bool>> desc");
        assert_eq!(tok, "array<int, array<string, bool>>");
    }

    #[test]
    fn phpdoc_type_end_no_description() {
        let (tok, _) = split_type_token("int");
        assert_eq!(tok, "int");
    }

    #[test]
    fn phpdoc_type_end_generic_no_description() {
        let (tok, _) = split_type_token("array<int, string>");
        assert_eq!(tok, "array<int, string>");
    }

    // ── build_update_return_type_edits ─────────────────────────────

    #[test]
    fn update_return_type_simple_changes_native_only() {
        // Simple type (no generics) — only native type hint changes.
        let content = "<?php\nfunction foo(): string {\n    return 42;\n}\n";
        let edits = build_update_return_type_edits(content, 2, &PhpType::parse("int")).unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, ": int");
    }

    #[test]
    fn update_return_type_generic_keeps_native_adds_docblock() {
        // Generic type — native stays `array`, docblock gets `array<int, int>`.
        let content = "<?php\nfunction foo(): array {\n    return [1, 2, 3];\n}\n";
        let edits =
            build_update_return_type_edits(content, 2, &PhpType::parse("array<int, int>")).unwrap();
        // Should only have docblock edit (native `array` already matches).
        assert_eq!(edits.len(), 1);
        assert!(
            edits[0].new_text.contains("@return array<int, int>"),
            "should create @return with generic type: {:?}",
            edits[0].new_text
        );
    }

    #[test]
    fn update_return_type_generic_changes_native_and_docblock() {
        // Native type differs AND has generics.
        let content = "<?php\nfunction foo(): string {\n    return [1, 2, 3];\n}\n";
        let edits =
            build_update_return_type_edits(content, 2, &PhpType::parse("array<int, int>")).unwrap();
        assert_eq!(edits.len(), 2);
        // One edit for the native type, one for the docblock.
        let type_edit = edits.iter().find(|e| e.new_text == ": array").unwrap();
        assert!(type_edit.range.start.line == 1);
        let doc_edit = edits
            .iter()
            .find(|e| e.new_text.contains("@return array<int, int>"))
            .unwrap();
        assert!(doc_edit.range.start.line == 1); // inserted before function
    }

    #[test]
    fn update_return_type_replaces_existing_generic_return_tag() {
        // Existing @return with generics — should be fully replaced.
        let content = "<?php\n/**\n * @return array<int, string>\n */\nfunction foo(): array {\n    return [1, 2, 3];\n}\n";
        let edits =
            build_update_return_type_edits(content, 5, &PhpType::parse("array<int, int>")).unwrap();
        assert_eq!(edits.len(), 1);
        let edit = &edits[0];
        assert!(
            edit.new_text.contains("@return array<int, int>"),
            "should replace generic type: {}",
            edit.new_text
        );
        // Old type should not remain.
        assert!(
            !edit.new_text.contains("string>"),
            "old generic params should be gone: {}",
            edit.new_text
        );
    }

    #[test]
    fn update_return_type_preserves_description_with_generics() {
        let content = "<?php\n/**\n * @return array<int, string> The data\n */\nfunction foo(): array {\n    return [1, 2, 3];\n}\n";
        let edits =
            build_update_return_type_edits(content, 5, &PhpType::parse("array<int, int>")).unwrap();
        assert_eq!(edits.len(), 1);
        assert!(
            edits[0].new_text.contains("@return array<int, int>"),
            "should have new type: {}",
            edits[0].new_text
        );
        assert!(
            edits[0].new_text.contains("The data"),
            "should preserve description: {}",
            edits[0].new_text
        );
    }

    #[test]
    fn update_return_type_returns_none_when_already_correct() {
        let content = "<?php\nfunction foo(): int {\n    return 42;\n}\n";
        assert!(build_update_return_type_edits(content, 2, &PhpType::parse("int")).is_none());
    }
}
