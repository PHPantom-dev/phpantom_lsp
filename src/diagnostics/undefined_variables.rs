//! Undefined variable diagnostics.
//!
//! Walk each function/method/closure body in the file and flag every
//! variable read that has no prior definition (assignment, parameter,
//! foreach binding, catch binding, `global`, `static`, `use()` clause,
//! or `list()`/`[…]` destructuring) in the same scope.
//!
//! Diagnostics use `Severity::Error` because accessing an undefined
//! variable is a runtime notice/warning (and `ErrorException` in strict
//! setups).  This is the single most impactful diagnostic for catching
//! typos in variable names.
//!
//! ## Implementation
//!
//! A variable read is flagged only when no write of the same name exists
//! at a **lower byte offset** in the same frame.  Writes inside any
//! control-flow branch (if/else, switch, try/catch) still count, so the
//! analysis is conservative about branches but strict about source order:
//! it catches the common "used before assigned" typo while avoiding false
//! positives from branch-dependent definitions.  `/** @var Type $var */`
//! annotations are treated as a write at the annotation's position and are
//! scoped to the frame they appear in.
//!
//! ## Suppression / false-positive avoidance
//!
//! The following patterns suppress the diagnostic for a variable:
//!
//! - **Superglobals** (`$_GET`, `$_POST`, `$_SERVER`, etc.) and
//!   `$this` are always considered defined.
//! - **`isset($var)` / `empty($var)`** — the variable is being
//!   guarded, not used.  Reads inside `isset()` and `empty()` are
//!   suppressed.
//! - **`compact('var')`** — `$var` is referenced by string name.
//!   All variable names mentioned in `compact()` calls are treated
//!   as defined.
//! - **`extract($array)`** — any variable could be defined; skip the
//!   entire function body.
//! - **`$$dynamic`** — variable variables make static analysis
//!   unsound; skip the entire function body.
//! - **`@$var`** — the error suppression operator signals intentional
//!   use of a potentially undefined variable.
//! - **`unset($var)`** — the variable is being destroyed, not read.
//!   `unset()` itself should not flag the variable.
//! - **`@var` annotation** — a `/** @var Type $var */` comment on
//!   the preceding line means the developer asserts the variable
//!   exists.
//! - **`$this`** inside a non-static method or closure — always
//!   defined.
//! - **`$this`** inside a static method or top-level code — flagged
//!   separately by other tools; we skip it.

use std::collections::HashSet;

use mago_span::HasSpan;
use mago_syntax::cst::*;
use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::parser::with_parsed_program;
use crate::scope_collector::{
    AccessKind, ByRefCallKind, ByRefResolver, FrameKind, ScopeMap,
    collect_function_scope_with_kind_and_resolver, collect_function_scope_with_resolver,
};

use super::helpers::make_diagnostic;

/// Diagnostic code used for undefined-variable diagnostics so that
/// code actions can match on it.
pub(crate) const UNKNOWN_VARIABLE_CODE: &str = "unknown_variable";

/// PHP superglobals and auto-defined variables that are always in scope.
const SUPERGLOBALS: &[&str] = &[
    "$_GET",
    "$_POST",
    "$_SERVER",
    "$_REQUEST",
    "$_SESSION",
    "$_COOKIE",
    "$_FILES",
    "$_ENV",
    "$GLOBALS",
    "$argc",
    "$argv",
    "$http_response_header",
    "$php_errormsg",
];

impl Backend {
    /// Collect undefined-variable diagnostics for a single file.
    ///
    /// Appends diagnostics to `out`.  The caller is responsible for
    /// publishing them via `textDocument/publishDiagnostics`.
    pub fn collect_undefined_variable_diagnostics(
        &self,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        // Gather file-level context for FQN resolution of function and
        // class names inside the by-ref resolver.
        let file_use_map: std::collections::HashMap<String, String> = self.file_use_map(uri);
        let file_namespace: Option<String> = self.first_file_namespace(uri);

        // Build a by-ref resolver that uses Backend to look up function
        // and method signatures.  This lets the scope collector mark
        // by-ref arguments as writes for user-defined functions, static
        // methods, and constructors — not just the hardcoded table.
        let resolver: ByRefResolver<'_> = &|call_kind: &ByRefCallKind<'_>| {
            self.resolve_by_ref_positions(call_kind, &file_use_map, &file_namespace)
        };

        with_parsed_program(content, "unknown_variable", |program, content| {
            let mut ctx = DiagnosticCtx {
                backend: self,
                uri,
                content,
                diagnostics: Vec::new(),
            };

            for stmt in program.statements.iter() {
                collect_from_statement(stmt, &mut ctx, Some(&resolver));
            }

            out.extend(ctx.diagnostics);
        });
    }

    /// Look up which parameter positions are by-reference for a given call.
    ///
    /// Returns `Some(vec![...])` with 0-based argument positions that are
    /// by-reference, or `None` if the callee cannot be resolved.
    fn resolve_by_ref_positions(
        &self,
        call_kind: &ByRefCallKind<'_>,
        file_use_map: &std::collections::HashMap<String, String>,
        file_namespace: &Option<String>,
    ) -> Option<Vec<usize>> {
        match call_kind {
            ByRefCallKind::Function(name) => {
                // Build FQN candidates: the raw name, plus the
                // namespace-qualified name if the file has a namespace.
                let fqn = crate::util::resolve_to_fqn(name, file_use_map, file_namespace);
                let mut candidates: Vec<&str> = vec![*name];
                if fqn != *name {
                    candidates.push(&fqn);
                }
                let func_info = self.find_or_load_function(&candidates)?;
                let positions: Vec<usize> = func_info
                    .parameters
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| p.is_reference)
                    .map(|(i, _)| i)
                    .collect();
                Some(positions)
            }
            ByRefCallKind::StaticMethod(class_name, method_name) => {
                let fqn = crate::util::resolve_to_fqn(class_name, file_use_map, file_namespace);
                let cls = self
                    .find_or_load_class(&fqn)
                    .or_else(|| self.find_or_load_class(class_name))?;
                let merged = crate::inheritance::resolve_class_with_inheritance(&cls, &|name| {
                    self.find_or_load_class(name)
                });
                let method = merged.get_method(method_name)?;
                let positions: Vec<usize> = method
                    .parameters
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| p.is_reference)
                    .map(|(i, _)| i)
                    .collect();
                Some(positions)
            }
            ByRefCallKind::Constructor(class_name) => {
                let fqn = crate::util::resolve_to_fqn(class_name, file_use_map, file_namespace);
                let cls = self
                    .find_or_load_class(&fqn)
                    .or_else(|| self.find_or_load_class(class_name))?;
                let merged = crate::inheritance::resolve_class_with_inheritance(&cls, &|name| {
                    self.find_or_load_class(name)
                });
                let ctor = merged.get_method("__construct")?;
                let positions: Vec<usize> = ctor
                    .parameters
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| p.is_reference)
                    .map(|(i, _)| i)
                    .collect();
                Some(positions)
            }
            ByRefCallKind::InstanceMethod(class_name, method_name) => {
                let fqn = crate::util::resolve_to_fqn(class_name, file_use_map, file_namespace);
                let cls = self
                    .find_or_load_class(&fqn)
                    .or_else(|| self.find_or_load_class(class_name))?;
                let merged = crate::inheritance::resolve_class_with_inheritance(&cls, &|name| {
                    self.find_or_load_class(name)
                });
                let method = merged.get_method(method_name)?;
                let positions: Vec<usize> = method
                    .parameters
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| p.is_reference)
                    .map(|(i, _)| i)
                    .collect();
                Some(positions)
            }
        }
    }
}

// ─── Internal context ───────────────────────────────────────────────────────

/// Collects diagnostics while walking the AST.
struct DiagnosticCtx<'a> {
    backend: &'a Backend,
    uri: &'a str,
    content: &'a str,
    diagnostics: Vec<Diagnostic>,
}

// ─── AST walking — find all function/method/closure bodies ──────────────────

/// Walk a top-level statement, recursing into namespace blocks,
/// class declarations, and function bodies.
fn collect_from_statement(
    stmt: &Statement<'_>,
    ctx: &mut DiagnosticCtx<'_>,
    resolver: Option<ByRefResolver<'_>>,
) {
    match stmt {
        Statement::Function(func) => {
            let body_start = func.body.left_brace.start.offset;
            let body_end = func.body.right_brace.end.offset;
            let scope = collect_function_scope_with_resolver(
                &func.parameter_list,
                func.body.statements.as_slice(),
                body_start,
                body_end,
                resolver,
            );
            check_scope(
                &scope,
                func.body.statements.as_slice(),
                ctx,
                false, // not a method
            );
        }
        Statement::Class(class) => {
            let class_name = crate::atom::bytes_to_str(class.name.value).to_string();
            collect_from_class_members(class.members.as_slice(), ctx, resolver, Some(&class_name));
        }
        Statement::Trait(tr) => {
            let trait_name = crate::atom::bytes_to_str(tr.name.value).to_string();
            collect_from_class_members(tr.members.as_slice(), ctx, resolver, Some(&trait_name));
        }
        Statement::Enum(en) => {
            let enum_name = crate::atom::bytes_to_str(en.name.value).to_string();
            collect_from_class_members(en.members.as_slice(), ctx, resolver, Some(&enum_name));
        }
        Statement::Interface(_) => {
            // Interfaces don't have method bodies.
        }
        Statement::Namespace(ns) => {
            for inner in ns.statements().iter() {
                collect_from_statement(inner, ctx, resolver);
            }
        }
        // Top-level code (outside any function/class).
        _ => {
            // We don't diagnose top-level code because PHP's global
            // scope has too many implicit variable definitions
            // (include/require, extract in bootstrap files, etc.).
        }
    }
}

/// Walk class-like members to find method bodies.
fn collect_from_class_members(
    members: &[ClassLikeMember<'_>],
    ctx: &mut DiagnosticCtx<'_>,
    resolver: Option<ByRefResolver<'_>>,
    class_name: Option<&str>,
) {
    for member in members.iter() {
        if let ClassLikeMember::Method(method) = member
            && let MethodBody::Concrete(block) = &method.body
        {
            let body_start = block.left_brace.start.offset;
            let body_end = block.right_brace.end.offset;

            let is_static = method
                .modifiers
                .iter()
                .any(|m| matches!(m, Modifier::Static(_)));

            let scope = collect_function_scope_with_kind_and_resolver(
                &method.parameter_list,
                block.statements.as_slice(),
                body_start,
                body_end,
                FrameKind::Method,
                resolver,
                class_name.map(|s| s.to_string()),
            );

            check_scope(&scope, block.statements.as_slice(), ctx, !is_static);
        }
    }
}

// ─── Scope analysis ─────────────────────────────────────────────────────────

/// Check a single scope (function/method body) for undefined variable reads.
///
/// For each variable read, we check whether the variable has been
/// written (assigned, declared as a parameter, etc.) at a **lower byte
/// offset** in the same frame.  Writes inside control-flow branches
/// (if/else, switch, try/catch) still count — we are conservative
/// about branches but strict about source order.  This catches the
/// common "used before assigned" typo while still avoiding false
/// positives from branch-dependent definitions.
fn check_scope(
    scope: &ScopeMap,
    statements: &[Statement<'_>],
    ctx: &mut DiagnosticCtx<'_>,
    this_is_defined: bool,
) {
    // Bail out early if the function uses features that make static
    // analysis unsound.
    if has_dynamic_variables(statements) || has_extract_call(statements) {
        return;
    }

    // Collect variable names referenced by compact() calls — these
    // variables are used by string name and should be treated as
    // defined.
    let compact_vars = collect_compact_vars(statements);

    // Collect variable names annotated with `/** @var Type $var */`
    // inline docblocks, each with the byte offset of its `$` sigil so it
    // can be treated as a scoped, source-ordered write rather than a
    // file-wide "always defined" name.
    let var_annotated = collect_var_annotations(ctx.content);

    // Collect byte offsets suppressed by the `@` error control
    // operator (e.g. `@$var`).
    let error_suppressed_offsets = collect_error_suppressed_offsets(statements);

    // Collect byte offsets of variables inside `isset()` and `empty()`.
    let guarded_offsets = collect_guarded_offsets(statements);

    // Bail out if there are no frames at all.
    if scope.frames.is_empty() {
        return;
    }

    // Build a set of "always-defined" names that do not require a
    // prior write: superglobals, compact-referenced vars, @var
    // annotations, and optionally $this.
    let mut always_defined: HashSet<&str> = HashSet::new();
    for sg in SUPERGLOBALS {
        always_defined.insert(sg);
    }
    if this_is_defined {
        always_defined.insert("$this");
    }
    for cv in &compact_vars {
        always_defined.insert(cv.as_str());
    }

    // Pre-compute the "own writes" for each frame: writes that are
    // directly inside the frame (not inside a nested sub-frame).
    let frame_own_writes: Vec<Vec<(&str, u32)>> = scope
        .frames
        .iter()
        .map(|frame| {
            let mut writes: Vec<(&str, u32)> = Vec::new();
            // Parameters (offset 0 = always before any read).
            for param in &frame.parameters {
                writes.push((param.as_str(), 0));
            }
            // Writes inside the frame body (excluding nested frames).
            for access in &scope.accesses {
                if !matches!(access.kind, AccessKind::Write | AccessKind::ReadWrite) {
                    continue;
                }
                if access.offset >= frame.start
                    && access.offset <= frame.end
                    && !is_in_nested_frame(access.offset, frame, &scope.frames)
                {
                    writes.push((access.name.as_str(), access.offset));
                }
            }
            // `/** @var Type $var */` annotations act as a write at the
            // annotation's offset, but only within the frame they appear in.
            for (name, offset) in &var_annotated {
                if *offset >= frame.start
                    && *offset <= frame.end
                    && !is_in_nested_frame(*offset, frame, &scope.frames)
                {
                    writes.push((name.as_str(), *offset));
                }
            }
            writes
        })
        .collect();

    // Process each frame independently.
    for (frame_idx, frame) in scope.frames.iter().enumerate() {
        // Build the list of writes visible to this frame by walking
        // up the parent-frame chain.  This correctly handles
        // arbitrary nesting depths (e.g. arrow fn inside closure
        // inside method, catch block inside closure, etc.).
        //
        // Visibility rules per frame kind:
        // - **Outermost / TopLevel / Function / Method**: own writes only
        // - **ArrowFunction / Catch**: parent's visible writes + own writes
        // - **Closure**: own writes only (captures are already recorded
        //   as Write accesses at body_start by the scope collector)
        let visible_writes = build_visible_writes(frame_idx, &scope.frames, &frame_own_writes);

        // Check reads: for each read, verify that a write of the same
        // name exists at a lower offset (or the name is always-defined).
        let frame_writes = &visible_writes;
        for access in &scope.accesses {
            if access.offset < frame.start || access.offset > frame.end {
                continue;
            }

            // Skip accesses inside nested frames.
            if is_in_nested_frame(access.offset, frame, &scope.frames) {
                continue;
            }

            if !matches!(access.kind, AccessKind::Read) {
                continue;
            }

            // Skip pseudo-variables.
            if access.name == "self" || access.name == "static" || access.name == "parent" {
                continue;
            }

            // Skip $this — even if not "defined", we don't flag it
            // (static methods will have $this reads flagged by other tools).
            if access.name == "$this" {
                continue;
            }

            // Skip if this read is guarded by isset() or empty().
            if guarded_offsets.contains(&access.offset) {
                continue;
            }

            // Skip if this read is under the @ error suppression operator.
            if error_suppressed_offsets.contains(&access.offset) {
                continue;
            }

            // Skip always-defined names.
            if always_defined.contains(access.name.as_str()) {
                continue;
            }

            // Check if any write of this variable exists at a lower
            // offset.  Parameters use offset 0 so they always qualify.
            let has_prior_write = frame_writes
                .iter()
                .any(|(name, off)| *name == access.name && *off < access.offset);

            if has_prior_write {
                continue;
            }

            // Emit diagnostic.
            let var_len = access.name.len();
            let range = match ctx.backend.offset_range_to_lsp_range(
                ctx.uri,
                ctx.content,
                access.offset as usize,
                access.offset as usize + var_len,
            ) {
                Some(r) => r,
                None => continue,
            };

            let message = format!("Undefined variable '{}'", access.name);

            ctx.diagnostics.push(make_diagnostic(
                range,
                DiagnosticSeverity::ERROR,
                UNKNOWN_VARIABLE_CODE,
                message,
            ));
        }
    }
}

/// Check whether a variable access at `offset` is inside a nested
/// frame (closure, arrow function) relative to the given `frame`.
/// Catch blocks are not treated as nested frames for this purpose
/// because variables defined in catch blocks leak into the enclosing
/// scope.
fn is_in_nested_frame(
    offset: u32,
    frame: &crate::scope_collector::Frame,
    frames: &[crate::scope_collector::Frame],
) -> bool {
    frames.iter().any(|f| {
        f.start > frame.start
            && f.end < frame.end
            && offset >= f.start
            && offset <= f.end
            && f.kind != FrameKind::Catch
    })
}

/// Find the index of the parent frame for `frame_idx`.
///
/// The parent is the smallest frame that strictly contains the given
/// frame.  Returns `None` for the outermost frame.
fn find_parent_frame_idx(
    frame_idx: usize,
    frames: &[crate::scope_collector::Frame],
) -> Option<usize> {
    let frame = &frames[frame_idx];
    let mut best: Option<usize> = None;
    for (i, candidate) in frames.iter().enumerate() {
        if i == frame_idx {
            continue;
        }
        // candidate must strictly contain frame
        if candidate.start <= frame.start
            && candidate.end >= frame.end
            && !(candidate.start == frame.start && candidate.end == frame.end)
        {
            match best {
                None => best = Some(i),
                Some(prev) => {
                    let prev_frame = &frames[prev];
                    // Pick the tighter (smaller) enclosing frame.
                    if (candidate.end - candidate.start) < (prev_frame.end - prev_frame.start) {
                        best = Some(i);
                    }
                }
            }
        }
    }
    best
}

/// Build the set of writes visible to the frame at `frame_idx` by
/// walking up the parent chain.
///
/// - **Arrow functions and catch blocks** inherit all writes visible
///   to their parent, plus their own direct writes.
/// - **Closures** see only their own direct writes (captures are
///   already recorded as Write accesses by the scope collector).
/// - **Outermost / function / method** frames see only their own
///   direct writes.
fn build_visible_writes<'a>(
    frame_idx: usize,
    frames: &[crate::scope_collector::Frame],
    frame_own_writes: &[Vec<(&'a str, u32)>],
) -> Vec<(&'a str, u32)> {
    let frame = &frames[frame_idx];
    let own = &frame_own_writes[frame_idx];

    match frame.kind {
        FrameKind::ArrowFunction | FrameKind::Catch => {
            // Inherit parent's visible writes, then add our own.
            let parent_writes = match find_parent_frame_idx(frame_idx, frames) {
                Some(parent_idx) => build_visible_writes(parent_idx, frames, frame_own_writes),
                None => Vec::new(),
            };
            let mut combined = parent_writes;
            combined.extend_from_slice(own);
            combined
        }
        _ => {
            // Closures, outermost, functions, methods: own writes only.
            own.clone()
        }
    }
}

// ─── Dynamic variable / extract detection ───────────────────────────────────

/// Returns `true` if the statements contain variable variables (`$$x`)
/// anywhere in the function body.
fn has_dynamic_variables(statements: &[Statement<'_>]) -> bool {
    for stmt in statements {
        if stmt_has_dynamic_var(stmt) {
            return true;
        }
    }
    false
}

fn stmt_has_dynamic_var(stmt: &Statement<'_>) -> bool {
    match stmt {
        Statement::Expression(es) => expr_has_dynamic_var(es.expression),
        Statement::Return(ret) => ret.value.is_some_and(|v| expr_has_dynamic_var(v)),
        Statement::Echo(echo) => echo.values.iter().any(|v| expr_has_dynamic_var(v)),
        Statement::If(if_stmt) => {
            if expr_has_dynamic_var(if_stmt.condition) {
                return true;
            }
            match &if_stmt.body {
                IfBody::Statement(body) => {
                    if stmt_has_dynamic_var(body.statement) {
                        return true;
                    }
                    for clause in body.else_if_clauses.iter() {
                        if expr_has_dynamic_var(clause.condition)
                            || stmt_has_dynamic_var(clause.statement)
                        {
                            return true;
                        }
                    }
                    if let Some(ref el) = body.else_clause
                        && stmt_has_dynamic_var(el.statement)
                    {
                        return true;
                    }
                }
                IfBody::ColonDelimited(body) => {
                    for s in body.statements.iter() {
                        if stmt_has_dynamic_var(s) {
                            return true;
                        }
                    }
                    for clause in body.else_if_clauses.iter() {
                        if expr_has_dynamic_var(clause.condition) {
                            return true;
                        }
                        for s in clause.statements.iter() {
                            if stmt_has_dynamic_var(s) {
                                return true;
                            }
                        }
                    }
                    if let Some(ref el) = body.else_clause {
                        for s in el.statements.iter() {
                            if stmt_has_dynamic_var(s) {
                                return true;
                            }
                        }
                    }
                }
            }
            false
        }
        Statement::Foreach(foreach) => {
            expr_has_dynamic_var(foreach.expression)
                || match &foreach.body {
                    ForeachBody::Statement(s) => stmt_has_dynamic_var(s),
                    ForeachBody::ColonDelimited(b) => {
                        b.statements.iter().any(|s| stmt_has_dynamic_var(s))
                    }
                }
        }
        Statement::While(w) => {
            expr_has_dynamic_var(w.condition)
                || match &w.body {
                    WhileBody::Statement(s) => stmt_has_dynamic_var(s),
                    WhileBody::ColonDelimited(b) => {
                        b.statements.iter().any(|s| stmt_has_dynamic_var(s))
                    }
                }
        }
        Statement::DoWhile(dw) => {
            stmt_has_dynamic_var(dw.statement) || expr_has_dynamic_var(dw.condition)
        }
        Statement::For(for_stmt) => {
            for_stmt
                .initializations
                .iter()
                .any(|e| expr_has_dynamic_var(e))
                || for_stmt.conditions.iter().any(|e| expr_has_dynamic_var(e))
                || for_stmt.increments.iter().any(|e| expr_has_dynamic_var(e))
                || match &for_stmt.body {
                    ForBody::Statement(s) => stmt_has_dynamic_var(s),
                    ForBody::ColonDelimited(b) => {
                        b.statements.iter().any(|s| stmt_has_dynamic_var(s))
                    }
                }
        }
        Statement::Switch(sw) => {
            expr_has_dynamic_var(sw.expression)
                || sw.body.cases().iter().any(|c| match c {
                    SwitchCase::Expression(sc) => {
                        expr_has_dynamic_var(sc.expression)
                            || sc.statements.iter().any(|s| stmt_has_dynamic_var(s))
                    }
                    SwitchCase::Default(dc) => {
                        dc.statements.iter().any(|s| stmt_has_dynamic_var(s))
                    }
                })
        }
        Statement::Try(try_stmt) => {
            try_stmt
                .block
                .statements
                .iter()
                .any(|s| stmt_has_dynamic_var(s))
                || try_stmt
                    .catch_clauses
                    .iter()
                    .any(|c| c.block.statements.iter().any(|s| stmt_has_dynamic_var(s)))
                || try_stmt
                    .finally_clause
                    .as_ref()
                    .is_some_and(|f| f.block.statements.iter().any(|s| stmt_has_dynamic_var(s)))
        }
        Statement::Block(block) => block.statements.iter().any(|s| stmt_has_dynamic_var(s)),
        Statement::Unset(_) => false,
        Statement::Global(_) => false,
        Statement::Static(_) => false,
        _ => false,
    }
}

fn expr_has_dynamic_var(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::Variable(Variable::Indirect(_)) => true,
        Expression::Variable(Variable::Nested(_)) => true,
        Expression::Variable(_) => false,
        Expression::Assignment(a) => expr_has_dynamic_var(a.lhs) || expr_has_dynamic_var(a.rhs),
        Expression::Binary(b) => expr_has_dynamic_var(b.lhs) || expr_has_dynamic_var(b.rhs),
        Expression::UnaryPrefix(u) => expr_has_dynamic_var(u.operand),
        Expression::UnaryPostfix(u) => expr_has_dynamic_var(u.operand),
        Expression::Parenthesized(p) => expr_has_dynamic_var(p.expression),
        Expression::Call(call) => match call {
            Call::Function(fc) => {
                expr_has_dynamic_var(fc.function)
                    || fc
                        .argument_list
                        .arguments
                        .iter()
                        .any(|a| expr_has_dynamic_var(a.value()))
            }
            Call::Method(mc) => {
                expr_has_dynamic_var(mc.object)
                    || mc
                        .argument_list
                        .arguments
                        .iter()
                        .any(|a| expr_has_dynamic_var(a.value()))
            }
            Call::NullSafeMethod(mc) => {
                expr_has_dynamic_var(mc.object)
                    || mc
                        .argument_list
                        .arguments
                        .iter()
                        .any(|a| expr_has_dynamic_var(a.value()))
            }
            Call::StaticMethod(sc) => {
                expr_has_dynamic_var(sc.class)
                    || sc
                        .argument_list
                        .arguments
                        .iter()
                        .any(|a| expr_has_dynamic_var(a.value()))
            }
        },
        Expression::Access(access) => match access {
            Access::Property(pa) => expr_has_dynamic_var(pa.object),
            Access::NullSafeProperty(pa) => expr_has_dynamic_var(pa.object),
            Access::StaticProperty(spa) => expr_has_dynamic_var(spa.class),
            Access::ClassConstant(cca) => expr_has_dynamic_var(cca.class),
        },
        Expression::ArrayAccess(aa) => {
            expr_has_dynamic_var(aa.array) || expr_has_dynamic_var(aa.index)
        }
        Expression::Conditional(c) => {
            expr_has_dynamic_var(c.condition)
                || c.then.is_some_and(|t| expr_has_dynamic_var(t))
                || expr_has_dynamic_var(c.r#else)
        }
        Expression::Instantiation(inst) => {
            expr_has_dynamic_var(inst.class)
                || inst
                    .argument_list
                    .as_ref()
                    .is_some_and(|al| al.arguments.iter().any(|a| expr_has_dynamic_var(a.value())))
        }
        Expression::Array(arr) => arr.elements.iter().any(|e| array_elem_has_dynamic_var(e)),
        Expression::LegacyArray(arr) => arr.elements.iter().any(|e| array_elem_has_dynamic_var(e)),
        Expression::Throw(t) => expr_has_dynamic_var(t.exception),
        Expression::Clone(c) => expr_has_dynamic_var(c.object),
        Expression::Match(m) => {
            expr_has_dynamic_var(m.expression)
                || m.arms.iter().any(|arm| match arm {
                    MatchArm::Expression(ea) => {
                        ea.conditions.iter().any(|c| expr_has_dynamic_var(c))
                            || expr_has_dynamic_var(ea.expression)
                    }
                    MatchArm::Default(da) => expr_has_dynamic_var(da.expression),
                })
        }
        // Closures and arrow functions have their own scope — don't
        // recurse into them for dynamic variable detection.
        Expression::Closure(_) | Expression::ArrowFunction(_) => false,
        _ => false,
    }
}

fn array_elem_has_dynamic_var(elem: &ArrayElement<'_>) -> bool {
    match elem {
        ArrayElement::KeyValue(kv) => {
            expr_has_dynamic_var(kv.key) || expr_has_dynamic_var(kv.value)
        }
        ArrayElement::Value(v) => expr_has_dynamic_var(v.value),
        ArrayElement::Variadic(s) => expr_has_dynamic_var(s.value),
        ArrayElement::Missing(_) => false,
    }
}

/// Returns `true` if the statements contain a call to `extract()`.
fn has_extract_call(statements: &[Statement<'_>]) -> bool {
    for stmt in statements {
        if stmt_has_extract(stmt) {
            return true;
        }
    }
    false
}

fn stmt_has_extract(stmt: &Statement<'_>) -> bool {
    match stmt {
        Statement::Expression(es) => expr_has_extract(es.expression),
        Statement::Return(ret) => ret.value.is_some_and(|v| expr_has_extract(v)),
        Statement::Echo(echo) => echo.values.iter().any(|v| expr_has_extract(v)),
        Statement::If(if_stmt) => {
            if expr_has_extract(if_stmt.condition) {
                return true;
            }
            match &if_stmt.body {
                IfBody::Statement(body) => {
                    if stmt_has_extract(body.statement) {
                        return true;
                    }
                    for clause in body.else_if_clauses.iter() {
                        if expr_has_extract(clause.condition) || stmt_has_extract(clause.statement)
                        {
                            return true;
                        }
                    }
                    if let Some(ref el) = body.else_clause
                        && stmt_has_extract(el.statement)
                    {
                        return true;
                    }
                }
                IfBody::ColonDelimited(body) => {
                    for s in body.statements.iter() {
                        if stmt_has_extract(s) {
                            return true;
                        }
                    }
                    for clause in body.else_if_clauses.iter() {
                        if expr_has_extract(clause.condition) {
                            return true;
                        }
                        for s in clause.statements.iter() {
                            if stmt_has_extract(s) {
                                return true;
                            }
                        }
                    }
                    if let Some(ref el) = body.else_clause {
                        for s in el.statements.iter() {
                            if stmt_has_extract(s) {
                                return true;
                            }
                        }
                    }
                }
            }
            false
        }
        Statement::Foreach(foreach) => {
            expr_has_extract(foreach.expression)
                || match &foreach.body {
                    ForeachBody::Statement(s) => stmt_has_extract(s),
                    ForeachBody::ColonDelimited(b) => {
                        b.statements.iter().any(|s| stmt_has_extract(s))
                    }
                }
        }
        Statement::While(w) => {
            expr_has_extract(w.condition)
                || match &w.body {
                    WhileBody::Statement(s) => stmt_has_extract(s),
                    WhileBody::ColonDelimited(b) => {
                        b.statements.iter().any(|s| stmt_has_extract(s))
                    }
                }
        }
        Statement::DoWhile(dw) => stmt_has_extract(dw.statement) || expr_has_extract(dw.condition),
        Statement::For(for_stmt) => {
            for_stmt.initializations.iter().any(|e| expr_has_extract(e))
                || for_stmt.conditions.iter().any(|e| expr_has_extract(e))
                || for_stmt.increments.iter().any(|e| expr_has_extract(e))
                || match &for_stmt.body {
                    ForBody::Statement(s) => stmt_has_extract(s),
                    ForBody::ColonDelimited(b) => b.statements.iter().any(|s| stmt_has_extract(s)),
                }
        }
        Statement::Switch(sw) => {
            expr_has_extract(sw.expression)
                || sw.body.cases().iter().any(|c| match c {
                    SwitchCase::Expression(sc) => {
                        expr_has_extract(sc.expression)
                            || sc.statements.iter().any(|s| stmt_has_extract(s))
                    }
                    SwitchCase::Default(dc) => dc.statements.iter().any(|s| stmt_has_extract(s)),
                })
        }
        Statement::Try(try_stmt) => {
            try_stmt
                .block
                .statements
                .iter()
                .any(|s| stmt_has_extract(s))
                || try_stmt
                    .catch_clauses
                    .iter()
                    .any(|c| c.block.statements.iter().any(|s| stmt_has_extract(s)))
                || try_stmt
                    .finally_clause
                    .as_ref()
                    .is_some_and(|f| f.block.statements.iter().any(|s| stmt_has_extract(s)))
        }
        Statement::Block(block) => block.statements.iter().any(|s| stmt_has_extract(s)),
        _ => false,
    }
}

fn expr_has_extract(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::Call(Call::Function(fc)) => {
            if let Expression::Identifier(ident) = fc.function
                && ident.value().eq_ignore_ascii_case(b"extract")
            {
                return true;
            }
            // Check arguments recursively.
            fc.argument_list
                .arguments
                .iter()
                .any(|a| expr_has_extract(a.value()))
        }
        Expression::Assignment(a) => expr_has_extract(a.lhs) || expr_has_extract(a.rhs),
        Expression::Binary(b) => expr_has_extract(b.lhs) || expr_has_extract(b.rhs),
        Expression::UnaryPrefix(u) => expr_has_extract(u.operand),
        Expression::UnaryPostfix(u) => expr_has_extract(u.operand),
        Expression::Parenthesized(p) => expr_has_extract(p.expression),
        Expression::Conditional(c) => {
            expr_has_extract(c.condition)
                || c.then.is_some_and(|t| expr_has_extract(t))
                || expr_has_extract(c.r#else)
        }
        Expression::Call(Call::Method(mc)) => {
            expr_has_extract(mc.object)
                || mc
                    .argument_list
                    .arguments
                    .iter()
                    .any(|a| expr_has_extract(a.value()))
        }
        Expression::Call(Call::NullSafeMethod(mc)) => {
            expr_has_extract(mc.object)
                || mc
                    .argument_list
                    .arguments
                    .iter()
                    .any(|a| expr_has_extract(a.value()))
        }
        Expression::Call(Call::StaticMethod(sc)) => {
            expr_has_extract(sc.class)
                || sc
                    .argument_list
                    .arguments
                    .iter()
                    .any(|a| expr_has_extract(a.value()))
        }
        // Don't recurse into closures/arrow functions.
        Expression::Closure(_) | Expression::ArrowFunction(_) => false,
        _ => false,
    }
}

// ─── compact() variable collection ──────────────────────────────────────────

/// Collect variable names referenced by `compact('var1', 'var2', …)`
/// calls.  These variables are used by string name and should be
/// considered defined.
pub(crate) fn collect_compact_vars(statements: &[Statement<'_>]) -> HashSet<String> {
    let mut vars = HashSet::new();
    for stmt in statements {
        collect_compact_from_stmt(stmt, &mut vars);
    }
    vars
}

fn collect_compact_from_stmt(stmt: &Statement<'_>, vars: &mut HashSet<String>) {
    match stmt {
        Statement::Expression(es) => collect_compact_from_expr(es.expression, vars),
        Statement::Return(ret) => {
            if let Some(v) = ret.value {
                collect_compact_from_expr(v, vars);
            }
        }
        Statement::Echo(echo) => {
            for v in echo.values.iter() {
                collect_compact_from_expr(v, vars);
            }
        }
        Statement::If(if_stmt) => {
            collect_compact_from_expr(if_stmt.condition, vars);
            match &if_stmt.body {
                IfBody::Statement(body) => {
                    collect_compact_from_stmt(body.statement, vars);
                    for clause in body.else_if_clauses.iter() {
                        collect_compact_from_expr(clause.condition, vars);
                        collect_compact_from_stmt(clause.statement, vars);
                    }
                    if let Some(ref el) = body.else_clause {
                        collect_compact_from_stmt(el.statement, vars);
                    }
                }
                IfBody::ColonDelimited(body) => {
                    for s in body.statements.iter() {
                        collect_compact_from_stmt(s, vars);
                    }
                    for clause in body.else_if_clauses.iter() {
                        collect_compact_from_expr(clause.condition, vars);
                        for s in clause.statements.iter() {
                            collect_compact_from_stmt(s, vars);
                        }
                    }
                    if let Some(ref el) = body.else_clause {
                        for s in el.statements.iter() {
                            collect_compact_from_stmt(s, vars);
                        }
                    }
                }
            }
        }
        Statement::Foreach(foreach) => {
            collect_compact_from_expr(foreach.expression, vars);
            match &foreach.body {
                ForeachBody::Statement(s) => collect_compact_from_stmt(s, vars),
                ForeachBody::ColonDelimited(b) => {
                    for s in b.statements.iter() {
                        collect_compact_from_stmt(s, vars);
                    }
                }
            }
        }
        Statement::While(w) => {
            collect_compact_from_expr(w.condition, vars);
            match &w.body {
                WhileBody::Statement(s) => collect_compact_from_stmt(s, vars),
                WhileBody::ColonDelimited(b) => {
                    for s in b.statements.iter() {
                        collect_compact_from_stmt(s, vars);
                    }
                }
            }
        }
        Statement::DoWhile(dw) => {
            collect_compact_from_stmt(dw.statement, vars);
            collect_compact_from_expr(dw.condition, vars);
        }
        Statement::For(for_stmt) => {
            for e in for_stmt.initializations.iter() {
                collect_compact_from_expr(e, vars);
            }
            for e in for_stmt.conditions.iter() {
                collect_compact_from_expr(e, vars);
            }
            for e in for_stmt.increments.iter() {
                collect_compact_from_expr(e, vars);
            }
            match &for_stmt.body {
                ForBody::Statement(s) => collect_compact_from_stmt(s, vars),
                ForBody::ColonDelimited(b) => {
                    for s in b.statements.iter() {
                        collect_compact_from_stmt(s, vars);
                    }
                }
            }
        }
        Statement::Switch(sw) => {
            collect_compact_from_expr(sw.expression, vars);
            for case in sw.body.cases().iter() {
                match case {
                    SwitchCase::Expression(sc) => {
                        collect_compact_from_expr(sc.expression, vars);
                        for s in sc.statements.iter() {
                            collect_compact_from_stmt(s, vars);
                        }
                    }
                    SwitchCase::Default(dc) => {
                        for s in dc.statements.iter() {
                            collect_compact_from_stmt(s, vars);
                        }
                    }
                }
            }
        }
        Statement::Try(try_stmt) => {
            for s in try_stmt.block.statements.iter() {
                collect_compact_from_stmt(s, vars);
            }
            for catch in try_stmt.catch_clauses.iter() {
                for s in catch.block.statements.iter() {
                    collect_compact_from_stmt(s, vars);
                }
            }
            if let Some(ref finally) = try_stmt.finally_clause {
                for s in finally.block.statements.iter() {
                    collect_compact_from_stmt(s, vars);
                }
            }
        }
        Statement::Block(block) => {
            for s in block.statements.iter() {
                collect_compact_from_stmt(s, vars);
            }
        }
        _ => {}
    }
}

fn collect_compact_from_expr(expr: &Expression<'_>, vars: &mut HashSet<String>) {
    match expr {
        Expression::Call(Call::Function(fc)) => {
            if let Expression::Identifier(ident) = fc.function
                && ident.value().eq_ignore_ascii_case(b"compact")
            {
                // Each argument is a variable name (string literal) or
                // an array of names (possibly nested), matching the
                // forms compact() accepts.
                for arg in fc.argument_list.arguments.iter() {
                    collect_compact_name_from_arg(arg.value(), vars);
                }
            }
            // Also recurse into arguments for nested compact() calls.
            for arg in fc.argument_list.arguments.iter() {
                collect_compact_from_expr(arg.value(), vars);
            }
        }
        Expression::Assignment(a) => {
            collect_compact_from_expr(a.lhs, vars);
            collect_compact_from_expr(a.rhs, vars);
        }
        Expression::Binary(b) => {
            collect_compact_from_expr(b.lhs, vars);
            collect_compact_from_expr(b.rhs, vars);
        }
        Expression::Parenthesized(p) => collect_compact_from_expr(p.expression, vars),
        Expression::Conditional(c) => {
            collect_compact_from_expr(c.condition, vars);
            if let Some(t) = c.then {
                collect_compact_from_expr(t, vars);
            }
            collect_compact_from_expr(c.r#else, vars);
        }
        Expression::Call(Call::Method(mc)) => {
            collect_compact_from_expr(mc.object, vars);
            for arg in mc.argument_list.arguments.iter() {
                collect_compact_from_expr(arg.value(), vars);
            }
        }
        Expression::Call(Call::NullSafeMethod(mc)) => {
            collect_compact_from_expr(mc.object, vars);
            for arg in mc.argument_list.arguments.iter() {
                collect_compact_from_expr(arg.value(), vars);
            }
        }
        Expression::Call(Call::StaticMethod(sc)) => {
            collect_compact_from_expr(sc.class, vars);
            for arg in sc.argument_list.arguments.iter() {
                collect_compact_from_expr(arg.value(), vars);
            }
        }
        // Don't recurse into closures/arrow functions.
        _ => {}
    }
}

/// Collect variable names from a single `compact()` argument. A string
/// literal names a variable directly; an array literal is descended
/// into recursively so `compact(['a', ['b']])` collects both names.
fn collect_compact_name_from_arg(expr: &Expression<'_>, vars: &mut HashSet<String>) {
    match expr {
        Expression::Literal(Literal::String(s)) => {
            // `value` is the interpreted string content (without
            // quotes); fall back to `raw` and strip quotes manually
            // if `value` is `None`.
            let name: &str = if let Some(v) = s.value {
                crate::atom::bytes_to_str(v)
            } else {
                let raw = crate::atom::bytes_to_str(s.raw);
                raw.strip_prefix('\'')
                    .or_else(|| raw.strip_prefix('"'))
                    .and_then(|inner| inner.strip_suffix('\'').or_else(|| inner.strip_suffix('"')))
                    .unwrap_or(raw)
            };
            if !name.is_empty() {
                vars.insert(format!("${}", name));
            }
        }
        Expression::Array(arr) => {
            for elem in arr.elements.iter() {
                collect_compact_name_from_elem(elem, vars);
            }
        }
        Expression::LegacyArray(arr) => {
            for elem in arr.elements.iter() {
                collect_compact_name_from_elem(elem, vars);
            }
        }
        _ => {}
    }
}

/// Collect variable names from one element of an array passed to
/// `compact()`. Keys are ignored; values are names or nested arrays.
fn collect_compact_name_from_elem(elem: &ArrayElement<'_>, vars: &mut HashSet<String>) {
    match elem {
        ArrayElement::KeyValue(kv) => collect_compact_name_from_arg(kv.value, vars),
        ArrayElement::Value(v) => collect_compact_name_from_arg(v.value, vars),
        ArrayElement::Variadic(s) => collect_compact_name_from_arg(s.value, vars),
        ArrayElement::Missing(_) => {}
    }
}

// ─── get_defined_vars() detection ───────────────────────────────────────────

/// Returns true if the statements contain a call to `get_defined_vars()`.
/// When present in a scope, all variables defined in that scope are
/// considered used (e.g. for debug dumps), so unused-variable diagnostics
/// should be suppressed for them.
pub(crate) fn has_get_defined_vars(statements: &[Statement<'_>]) -> bool {
    for stmt in statements {
        if stmt_has_get_defined_vars(stmt) {
            return true;
        }
    }
    false
}

fn stmt_has_get_defined_vars(stmt: &Statement<'_>) -> bool {
    match stmt {
        Statement::Expression(es) => expr_has_get_defined_vars(es.expression),
        Statement::Return(ret) => ret.value.is_some_and(|v| expr_has_get_defined_vars(v)),
        Statement::Echo(echo) => echo.values.iter().any(|v| expr_has_get_defined_vars(v)),
        Statement::If(if_stmt) => {
            if expr_has_get_defined_vars(if_stmt.condition) {
                return true;
            }
            match &if_stmt.body {
                IfBody::Statement(body) => {
                    if stmt_has_get_defined_vars(body.statement) {
                        return true;
                    }
                    for clause in body.else_if_clauses.iter() {
                        if expr_has_get_defined_vars(clause.condition)
                            || stmt_has_get_defined_vars(clause.statement)
                        {
                            return true;
                        }
                    }
                    if let Some(ref el) = body.else_clause
                        && stmt_has_get_defined_vars(el.statement)
                    {
                        return true;
                    }
                }
                IfBody::ColonDelimited(body) => {
                    for s in body.statements.iter() {
                        if stmt_has_get_defined_vars(s) {
                            return true;
                        }
                    }
                    for clause in body.else_if_clauses.iter() {
                        if expr_has_get_defined_vars(clause.condition) {
                            return true;
                        }
                        for s in clause.statements.iter() {
                            if stmt_has_get_defined_vars(s) {
                                return true;
                            }
                        }
                    }
                    if let Some(ref el) = body.else_clause {
                        for s in el.statements.iter() {
                            if stmt_has_get_defined_vars(s) {
                                return true;
                            }
                        }
                    }
                }
            }
            false
        }
        Statement::Foreach(foreach) => {
            expr_has_get_defined_vars(foreach.expression)
                || match &foreach.body {
                    ForeachBody::Statement(s) => stmt_has_get_defined_vars(s),
                    ForeachBody::ColonDelimited(b) => {
                        b.statements.iter().any(|s| stmt_has_get_defined_vars(s))
                    }
                }
        }
        Statement::While(w) => {
            expr_has_get_defined_vars(w.condition)
                || match &w.body {
                    WhileBody::Statement(s) => stmt_has_get_defined_vars(s),
                    WhileBody::ColonDelimited(b) => {
                        b.statements.iter().any(|s| stmt_has_get_defined_vars(s))
                    }
                }
        }
        Statement::DoWhile(dw) => {
            stmt_has_get_defined_vars(dw.statement) || expr_has_get_defined_vars(dw.condition)
        }
        Statement::For(for_stmt) => {
            for_stmt
                .initializations
                .iter()
                .any(|e| expr_has_get_defined_vars(e))
                || for_stmt
                    .conditions
                    .iter()
                    .any(|e| expr_has_get_defined_vars(e))
                || for_stmt
                    .increments
                    .iter()
                    .any(|e| expr_has_get_defined_vars(e))
                || match &for_stmt.body {
                    ForBody::Statement(s) => stmt_has_get_defined_vars(s),
                    ForBody::ColonDelimited(b) => {
                        b.statements.iter().any(|s| stmt_has_get_defined_vars(s))
                    }
                }
        }
        Statement::Switch(sw) => {
            expr_has_get_defined_vars(sw.expression)
                || sw.body.cases().iter().any(|c| match c {
                    SwitchCase::Expression(sc) => {
                        expr_has_get_defined_vars(sc.expression)
                            || sc.statements.iter().any(|s| stmt_has_get_defined_vars(s))
                    }
                    SwitchCase::Default(dc) => {
                        dc.statements.iter().any(|s| stmt_has_get_defined_vars(s))
                    }
                })
        }
        Statement::Try(try_stmt) => {
            try_stmt
                .block
                .statements
                .iter()
                .any(|s| stmt_has_get_defined_vars(s))
                || try_stmt.catch_clauses.iter().any(|c| {
                    c.block
                        .statements
                        .iter()
                        .any(|s| stmt_has_get_defined_vars(s))
                })
                || try_stmt.finally_clause.as_ref().is_some_and(|f| {
                    f.block
                        .statements
                        .iter()
                        .any(|s| stmt_has_get_defined_vars(s))
                })
        }
        Statement::Block(block) => block
            .statements
            .iter()
            .any(|s| stmt_has_get_defined_vars(s)),
        _ => false,
    }
}

fn expr_has_get_defined_vars(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::Call(Call::Function(fc)) => {
            if let Expression::Identifier(ident) = fc.function
                && ident.value().eq_ignore_ascii_case(b"get_defined_vars")
            {
                return true;
            }
            expr_has_get_defined_vars(fc.function)
                // Recurse into arguments for nested calls.
                || fc
                    .argument_list
                    .arguments
                    .iter()
                    .any(|a| expr_has_get_defined_vars(a.value()))
        }
        Expression::Call(Call::Method(mc)) => {
            expr_has_get_defined_vars(mc.object)
                || mc
                    .argument_list
                    .arguments
                    .iter()
                    .any(|a| expr_has_get_defined_vars(a.value()))
        }
        Expression::Call(Call::NullSafeMethod(mc)) => {
            expr_has_get_defined_vars(mc.object)
                || mc
                    .argument_list
                    .arguments
                    .iter()
                    .any(|a| expr_has_get_defined_vars(a.value()))
        }
        Expression::Call(Call::StaticMethod(sc)) => {
            expr_has_get_defined_vars(sc.class)
                || sc
                    .argument_list
                    .arguments
                    .iter()
                    .any(|a| expr_has_get_defined_vars(a.value()))
        }
        Expression::Access(access) => match access {
            Access::Property(pa) => expr_has_get_defined_vars(pa.object),
            Access::NullSafeProperty(pa) => expr_has_get_defined_vars(pa.object),
            Access::StaticProperty(spa) => expr_has_get_defined_vars(spa.class),
            Access::ClassConstant(cca) => expr_has_get_defined_vars(cca.class),
        },
        Expression::ArrayAccess(aa) => {
            expr_has_get_defined_vars(aa.array) || expr_has_get_defined_vars(aa.index)
        }
        Expression::ArrayAppend(append) => expr_has_get_defined_vars(append.array),
        Expression::Array(arr) => arr
            .elements
            .iter()
            .any(|e| array_elem_has_get_defined_vars(e)),
        Expression::LegacyArray(arr) => arr
            .elements
            .iter()
            .any(|e| array_elem_has_get_defined_vars(e)),
        Expression::List(list) => list
            .elements
            .iter()
            .any(|e| array_elem_has_get_defined_vars(e)),
        Expression::Assignment(a) => {
            expr_has_get_defined_vars(a.lhs) || expr_has_get_defined_vars(a.rhs)
        }
        Expression::Binary(b) => {
            expr_has_get_defined_vars(b.lhs) || expr_has_get_defined_vars(b.rhs)
        }
        Expression::UnaryPrefix(u) => expr_has_get_defined_vars(u.operand),
        Expression::UnaryPostfix(u) => expr_has_get_defined_vars(u.operand),
        Expression::Parenthesized(p) => expr_has_get_defined_vars(p.expression),
        Expression::Conditional(c) => {
            expr_has_get_defined_vars(c.condition)
                || c.then.is_some_and(|t| expr_has_get_defined_vars(t))
                || expr_has_get_defined_vars(c.r#else)
        }
        Expression::Instantiation(inst) => {
            expr_has_get_defined_vars(inst.class)
                || inst.argument_list.as_ref().is_some_and(|al| {
                    al.arguments
                        .iter()
                        .any(|a| expr_has_get_defined_vars(a.value()))
                })
        }
        Expression::Throw(t) => expr_has_get_defined_vars(t.exception),
        Expression::Clone(c) => expr_has_get_defined_vars(c.object),
        Expression::Yield(yield_expr) => match yield_expr {
            Yield::Value(yv) => yv.value.is_some_and(expr_has_get_defined_vars),
            Yield::Pair(yp) => {
                expr_has_get_defined_vars(yp.key) || expr_has_get_defined_vars(yp.value)
            }
            Yield::From(yf) => expr_has_get_defined_vars(yf.iterator),
        },
        Expression::Match(m) => {
            expr_has_get_defined_vars(m.expression)
                || m.arms.iter().any(|arm| match arm {
                    MatchArm::Expression(ea) => {
                        ea.conditions.iter().any(|c| expr_has_get_defined_vars(c))
                            || expr_has_get_defined_vars(ea.expression)
                    }
                    MatchArm::Default(da) => expr_has_get_defined_vars(da.expression),
                })
        }
        Expression::Construct(construct) => match construct {
            Construct::Isset(isset) => isset.values.iter().any(|v| expr_has_get_defined_vars(v)),
            Construct::Empty(empty) => expr_has_get_defined_vars(empty.value),
            Construct::Eval(eval) => expr_has_get_defined_vars(eval.value),
            Construct::Include(inc) => expr_has_get_defined_vars(inc.value),
            Construct::IncludeOnce(inc) => expr_has_get_defined_vars(inc.value),
            Construct::Require(req) => expr_has_get_defined_vars(req.value),
            Construct::RequireOnce(req) => expr_has_get_defined_vars(req.value),
            Construct::Print(print) => expr_has_get_defined_vars(print.value),
            Construct::Exit(exit) => exit.arguments.as_ref().is_some_and(|args| {
                args.arguments
                    .iter()
                    .any(|a| expr_has_get_defined_vars(a.value()))
            }),
            Construct::Die(die) => die.arguments.as_ref().is_some_and(|args| {
                args.arguments
                    .iter()
                    .any(|a| expr_has_get_defined_vars(a.value()))
            }),
        },
        Expression::CompositeString(composite) => composite.parts().iter().any(|part| match part {
            StringPart::Expression(inner_expr) => expr_has_get_defined_vars(inner_expr),
            StringPart::BracedExpression(braced) => expr_has_get_defined_vars(braced.expression),
            StringPart::Literal(_) => false,
        }),
        Expression::Pipe(pipe) => {
            expr_has_get_defined_vars(pipe.input) || expr_has_get_defined_vars(pipe.callable)
        }
        Expression::PartialApplication(partial) => match partial {
            PartialApplication::Function(func_pa) => expr_has_get_defined_vars(func_pa.function),
            PartialApplication::Method(method_pa) => expr_has_get_defined_vars(method_pa.object),
            PartialApplication::StaticMethod(static_pa) => {
                expr_has_get_defined_vars(static_pa.class)
            }
        },
        Expression::AnonymousClass(anon) => anon.argument_list.as_ref().is_some_and(|args| {
            args.arguments
                .iter()
                .any(|a| a.value().is_some_and(expr_has_get_defined_vars))
        }),
        // Don't recurse into closures/arrow functions.
        Expression::Closure(_) | Expression::ArrowFunction(_) => false,
        _ => false,
    }
}

fn array_elem_has_get_defined_vars(elem: &ArrayElement<'_>) -> bool {
    match elem {
        ArrayElement::KeyValue(kv) => {
            expr_has_get_defined_vars(kv.key) || expr_has_get_defined_vars(kv.value)
        }
        ArrayElement::Value(v) => expr_has_get_defined_vars(v.value),
        ArrayElement::Variadic(s) => expr_has_get_defined_vars(s.value),
        ArrayElement::Missing(_) => false,
    }
}

// ─── @var annotation collection ─────────────────────────────────────────────

/// Scan the source text for `/** @var Type $varName */` inline
/// docblocks and return each declared variable name paired with the byte
/// offset of its `$` sigil.
///
/// The offset lets callers treat the annotation as a write at that
/// position so it (a) only defines the variable within the scope it
/// appears in, and (b) follows the same "prior write in source order"
/// rule as ordinary assignments.
fn collect_var_annotations(content: &str) -> Vec<(String, u32)> {
    let mut vars = Vec::new();
    // Look for patterns like: @var SomeType $varName
    // The regex-like scan: find `@var ` followed by a type, then `$name`.
    let mut line_start = 0usize;
    for line in content.lines() {
        // `lines()` strips the line terminator; track the running byte
        // offset so we can report absolute positions.
        let this_line_start = line_start;
        line_start += line.len() + 1; // +1 for the stripped '\n'

        if !line.contains("@var") {
            continue;
        }
        // Find `@var` and extract the variable name after the type.
        if let Some(var_pos) = line.find("@var") {
            let after_var_off = var_pos + 4;
            let after_var = &line[after_var_off..];
            let ws = after_var.len() - after_var.trim_start().len();
            let after_var = after_var.trim_start();
            // Skip the type (everything before the $).
            if let Some(dollar_pos) = after_var.find('$') {
                let var_part = &after_var[dollar_pos..];
                // Extract the variable name: $[a-zA-Z_][a-zA-Z0-9_]*
                let name_end = var_part
                    .char_indices()
                    .skip(1) // skip the $
                    .find(|(_, c)| !c.is_alphanumeric() && *c != '_')
                    .map(|(i, _)| i)
                    .unwrap_or(var_part.len());
                let var_name = &var_part[..name_end];
                // Trim trailing `*/` if present.
                let var_name = var_name.trim_end_matches("*/").trim();
                if var_name.len() > 1 {
                    let dollar_offset = this_line_start + after_var_off + ws + dollar_pos;
                    vars.push((var_name.to_string(), dollar_offset as u32));
                }
            }
        }
    }
    vars
}

// ─── Error suppression (@) offset collection ────────────────────────────────

/// Collect byte offsets of variable reads that are directly under the
/// `@` error suppression operator (e.g. `@$var`).
fn collect_error_suppressed_offsets(statements: &[Statement<'_>]) -> HashSet<u32> {
    let mut offsets = HashSet::new();
    for stmt in statements {
        collect_suppressed_from_stmt(stmt, &mut offsets);
    }
    offsets
}

fn collect_suppressed_from_stmt(stmt: &Statement<'_>, offsets: &mut HashSet<u32>) {
    match stmt {
        Statement::Expression(es) => collect_suppressed_from_expr(es.expression, false, offsets),
        Statement::Return(ret) => {
            if let Some(v) = ret.value {
                collect_suppressed_from_expr(v, false, offsets);
            }
        }
        Statement::Echo(echo) => {
            for v in echo.values.iter() {
                collect_suppressed_from_expr(v, false, offsets);
            }
        }
        Statement::If(if_stmt) => {
            collect_suppressed_from_expr(if_stmt.condition, false, offsets);
            match &if_stmt.body {
                IfBody::Statement(body) => {
                    collect_suppressed_from_stmt(body.statement, offsets);
                    for clause in body.else_if_clauses.iter() {
                        collect_suppressed_from_expr(clause.condition, false, offsets);
                        collect_suppressed_from_stmt(clause.statement, offsets);
                    }
                    if let Some(ref el) = body.else_clause {
                        collect_suppressed_from_stmt(el.statement, offsets);
                    }
                }
                IfBody::ColonDelimited(body) => {
                    for s in body.statements.iter() {
                        collect_suppressed_from_stmt(s, offsets);
                    }
                    for clause in body.else_if_clauses.iter() {
                        collect_suppressed_from_expr(clause.condition, false, offsets);
                        for s in clause.statements.iter() {
                            collect_suppressed_from_stmt(s, offsets);
                        }
                    }
                    if let Some(ref el) = body.else_clause {
                        for s in el.statements.iter() {
                            collect_suppressed_from_stmt(s, offsets);
                        }
                    }
                }
            }
        }
        Statement::Foreach(foreach) => {
            collect_suppressed_from_expr(foreach.expression, false, offsets);
            match &foreach.body {
                ForeachBody::Statement(s) => collect_suppressed_from_stmt(s, offsets),
                ForeachBody::ColonDelimited(b) => {
                    for s in b.statements.iter() {
                        collect_suppressed_from_stmt(s, offsets);
                    }
                }
            }
        }
        Statement::While(w) => {
            collect_suppressed_from_expr(w.condition, false, offsets);
            match &w.body {
                WhileBody::Statement(s) => collect_suppressed_from_stmt(s, offsets),
                WhileBody::ColonDelimited(b) => {
                    for s in b.statements.iter() {
                        collect_suppressed_from_stmt(s, offsets);
                    }
                }
            }
        }
        Statement::DoWhile(dw) => {
            collect_suppressed_from_stmt(dw.statement, offsets);
            collect_suppressed_from_expr(dw.condition, false, offsets);
        }
        Statement::For(for_stmt) => {
            for e in for_stmt.initializations.iter() {
                collect_suppressed_from_expr(e, false, offsets);
            }
            for e in for_stmt.conditions.iter() {
                collect_suppressed_from_expr(e, false, offsets);
            }
            for e in for_stmt.increments.iter() {
                collect_suppressed_from_expr(e, false, offsets);
            }
            match &for_stmt.body {
                ForBody::Statement(s) => collect_suppressed_from_stmt(s, offsets),
                ForBody::ColonDelimited(b) => {
                    for s in b.statements.iter() {
                        collect_suppressed_from_stmt(s, offsets);
                    }
                }
            }
        }
        Statement::Switch(sw) => {
            collect_suppressed_from_expr(sw.expression, false, offsets);
            for case in sw.body.cases().iter() {
                match case {
                    SwitchCase::Expression(sc) => {
                        collect_suppressed_from_expr(sc.expression, false, offsets);
                        for s in sc.statements.iter() {
                            collect_suppressed_from_stmt(s, offsets);
                        }
                    }
                    SwitchCase::Default(dc) => {
                        for s in dc.statements.iter() {
                            collect_suppressed_from_stmt(s, offsets);
                        }
                    }
                }
            }
        }
        Statement::Try(try_stmt) => {
            for s in try_stmt.block.statements.iter() {
                collect_suppressed_from_stmt(s, offsets);
            }
            for catch in try_stmt.catch_clauses.iter() {
                for s in catch.block.statements.iter() {
                    collect_suppressed_from_stmt(s, offsets);
                }
            }
            if let Some(ref finally) = try_stmt.finally_clause {
                for s in finally.block.statements.iter() {
                    collect_suppressed_from_stmt(s, offsets);
                }
            }
        }
        Statement::Block(block) => {
            for s in block.statements.iter() {
                collect_suppressed_from_stmt(s, offsets);
            }
        }
        _ => {}
    }
}

fn collect_suppressed_from_expr(
    expr: &Expression<'_>,
    under_error_control: bool,
    offsets: &mut HashSet<u32>,
) {
    match expr {
        Expression::UnaryPrefix(unary) if unary.operator.is_error_control() => {
            // The operand is under @.
            collect_suppressed_from_expr(unary.operand, true, offsets);
        }
        Expression::Variable(Variable::Direct(dv)) if under_error_control => {
            offsets.insert(dv.span().start.offset);
        }
        Expression::UnaryPrefix(unary) => {
            collect_suppressed_from_expr(unary.operand, under_error_control, offsets);
        }
        Expression::UnaryPostfix(unary) => {
            collect_suppressed_from_expr(unary.operand, under_error_control, offsets);
        }
        Expression::Assignment(a) => {
            collect_suppressed_from_expr(a.lhs, under_error_control, offsets);
            collect_suppressed_from_expr(a.rhs, under_error_control, offsets);
        }
        Expression::Binary(b) => {
            collect_suppressed_from_expr(b.lhs, under_error_control, offsets);
            collect_suppressed_from_expr(b.rhs, under_error_control, offsets);
        }
        Expression::Parenthesized(p) => {
            collect_suppressed_from_expr(p.expression, under_error_control, offsets);
        }
        Expression::Call(Call::Function(fc)) => {
            collect_suppressed_from_expr(fc.function, under_error_control, offsets);
            for arg in fc.argument_list.arguments.iter() {
                collect_suppressed_from_expr(arg.value(), under_error_control, offsets);
            }
        }
        Expression::Call(Call::Method(mc)) => {
            collect_suppressed_from_expr(mc.object, under_error_control, offsets);
            for arg in mc.argument_list.arguments.iter() {
                collect_suppressed_from_expr(arg.value(), under_error_control, offsets);
            }
        }
        Expression::Access(Access::Property(pa)) => {
            collect_suppressed_from_expr(pa.object, under_error_control, offsets);
        }
        Expression::Access(Access::NullSafeProperty(pa)) => {
            collect_suppressed_from_expr(pa.object, under_error_control, offsets);
        }
        Expression::ArrayAccess(aa) => {
            collect_suppressed_from_expr(aa.array, under_error_control, offsets);
            collect_suppressed_from_expr(aa.index, under_error_control, offsets);
        }
        Expression::Conditional(c) => {
            collect_suppressed_from_expr(c.condition, under_error_control, offsets);
            if let Some(t) = c.then {
                collect_suppressed_from_expr(t, under_error_control, offsets);
            }
            collect_suppressed_from_expr(c.r#else, under_error_control, offsets);
        }
        // Don't recurse into closures/arrow functions.
        _ => {}
    }
}

// ─── isset() / empty() guarded offset collection ───────────────────────────

/// Collect byte offsets of variable reads that appear directly inside
/// `isset()` or `empty()` calls.  These variables are being guarded,
/// not used.
fn collect_guarded_offsets(statements: &[Statement<'_>]) -> HashSet<u32> {
    let mut offsets = HashSet::new();
    for stmt in statements {
        collect_guarded_from_stmt(stmt, &mut offsets);
    }
    offsets
}

fn collect_guarded_from_stmt(stmt: &Statement<'_>, offsets: &mut HashSet<u32>) {
    match stmt {
        Statement::Expression(es) => collect_guarded_from_expr(es.expression, false, offsets),
        Statement::Return(ret) => {
            if let Some(v) = ret.value {
                collect_guarded_from_expr(v, false, offsets);
            }
        }
        Statement::Echo(echo) => {
            for v in echo.values.iter() {
                collect_guarded_from_expr(v, false, offsets);
            }
        }
        Statement::If(if_stmt) => {
            collect_guarded_from_expr(if_stmt.condition, false, offsets);
            match &if_stmt.body {
                IfBody::Statement(body) => {
                    collect_guarded_from_stmt(body.statement, offsets);
                    for clause in body.else_if_clauses.iter() {
                        collect_guarded_from_expr(clause.condition, false, offsets);
                        collect_guarded_from_stmt(clause.statement, offsets);
                    }
                    if let Some(ref el) = body.else_clause {
                        collect_guarded_from_stmt(el.statement, offsets);
                    }
                }
                IfBody::ColonDelimited(body) => {
                    for s in body.statements.iter() {
                        collect_guarded_from_stmt(s, offsets);
                    }
                    for clause in body.else_if_clauses.iter() {
                        collect_guarded_from_expr(clause.condition, false, offsets);
                        for s in clause.statements.iter() {
                            collect_guarded_from_stmt(s, offsets);
                        }
                    }
                    if let Some(ref el) = body.else_clause {
                        for s in el.statements.iter() {
                            collect_guarded_from_stmt(s, offsets);
                        }
                    }
                }
            }
        }
        Statement::Foreach(foreach) => {
            collect_guarded_from_expr(foreach.expression, false, offsets);
            match &foreach.body {
                ForeachBody::Statement(s) => collect_guarded_from_stmt(s, offsets),
                ForeachBody::ColonDelimited(b) => {
                    for s in b.statements.iter() {
                        collect_guarded_from_stmt(s, offsets);
                    }
                }
            }
        }
        Statement::While(w) => {
            collect_guarded_from_expr(w.condition, false, offsets);
            match &w.body {
                WhileBody::Statement(s) => collect_guarded_from_stmt(s, offsets),
                WhileBody::ColonDelimited(b) => {
                    for s in b.statements.iter() {
                        collect_guarded_from_stmt(s, offsets);
                    }
                }
            }
        }
        Statement::DoWhile(dw) => {
            collect_guarded_from_stmt(dw.statement, offsets);
            collect_guarded_from_expr(dw.condition, false, offsets);
        }
        Statement::For(for_stmt) => {
            for e in for_stmt.initializations.iter() {
                collect_guarded_from_expr(e, false, offsets);
            }
            for e in for_stmt.conditions.iter() {
                collect_guarded_from_expr(e, false, offsets);
            }
            for e in for_stmt.increments.iter() {
                collect_guarded_from_expr(e, false, offsets);
            }
            match &for_stmt.body {
                ForBody::Statement(s) => collect_guarded_from_stmt(s, offsets),
                ForBody::ColonDelimited(b) => {
                    for s in b.statements.iter() {
                        collect_guarded_from_stmt(s, offsets);
                    }
                }
            }
        }
        Statement::Switch(sw) => {
            collect_guarded_from_expr(sw.expression, false, offsets);
            for case in sw.body.cases().iter() {
                match case {
                    SwitchCase::Expression(sc) => {
                        collect_guarded_from_expr(sc.expression, false, offsets);
                        for s in sc.statements.iter() {
                            collect_guarded_from_stmt(s, offsets);
                        }
                    }
                    SwitchCase::Default(dc) => {
                        for s in dc.statements.iter() {
                            collect_guarded_from_stmt(s, offsets);
                        }
                    }
                }
            }
        }
        Statement::Try(try_stmt) => {
            for s in try_stmt.block.statements.iter() {
                collect_guarded_from_stmt(s, offsets);
            }
            for catch in try_stmt.catch_clauses.iter() {
                for s in catch.block.statements.iter() {
                    collect_guarded_from_stmt(s, offsets);
                }
            }
            if let Some(ref finally) = try_stmt.finally_clause {
                for s in finally.block.statements.iter() {
                    collect_guarded_from_stmt(s, offsets);
                }
            }
        }
        Statement::Block(block) => {
            for s in block.statements.iter() {
                collect_guarded_from_stmt(s, offsets);
            }
        }
        _ => {}
    }
}

fn collect_guarded_from_expr(
    expr: &Expression<'_>,
    inside_guard: bool,
    offsets: &mut HashSet<u32>,
) {
    match expr {
        Expression::Construct(Construct::Isset(isset)) => {
            // All variables inside isset() are guarded.
            for val in isset.values.iter() {
                collect_guard_targets(val, offsets);
            }
        }
        Expression::Construct(Construct::Empty(empty)) => {
            // The variable inside empty() is guarded.
            collect_guard_targets(empty.value, offsets);
        }
        Expression::UnaryPrefix(unary) => {
            collect_guarded_from_expr(unary.operand, inside_guard, offsets);
        }
        Expression::UnaryPostfix(unary) => {
            collect_guarded_from_expr(unary.operand, inside_guard, offsets);
        }
        Expression::Assignment(a) => {
            collect_guarded_from_expr(a.lhs, inside_guard, offsets);
            collect_guarded_from_expr(a.rhs, inside_guard, offsets);
        }
        Expression::Binary(b) => {
            collect_guarded_from_expr(b.lhs, inside_guard, offsets);
            collect_guarded_from_expr(b.rhs, inside_guard, offsets);
        }
        Expression::Parenthesized(p) => {
            collect_guarded_from_expr(p.expression, inside_guard, offsets);
        }
        Expression::Conditional(c) => {
            collect_guarded_from_expr(c.condition, inside_guard, offsets);
            if let Some(t) = c.then {
                collect_guarded_from_expr(t, inside_guard, offsets);
            }
            collect_guarded_from_expr(c.r#else, inside_guard, offsets);
        }
        Expression::Call(Call::Function(fc)) => {
            collect_guarded_from_expr(fc.function, inside_guard, offsets);
            for arg in fc.argument_list.arguments.iter() {
                collect_guarded_from_expr(arg.value(), inside_guard, offsets);
            }
        }
        Expression::Call(Call::Method(mc)) => {
            collect_guarded_from_expr(mc.object, inside_guard, offsets);
            for arg in mc.argument_list.arguments.iter() {
                collect_guarded_from_expr(arg.value(), inside_guard, offsets);
            }
        }
        Expression::Call(Call::NullSafeMethod(mc)) => {
            collect_guarded_from_expr(mc.object, inside_guard, offsets);
            for arg in mc.argument_list.arguments.iter() {
                collect_guarded_from_expr(arg.value(), inside_guard, offsets);
            }
        }
        Expression::Call(Call::StaticMethod(sc)) => {
            collect_guarded_from_expr(sc.class, inside_guard, offsets);
            for arg in sc.argument_list.arguments.iter() {
                collect_guarded_from_expr(arg.value(), inside_guard, offsets);
            }
        }
        Expression::Access(Access::Property(pa)) => {
            collect_guarded_from_expr(pa.object, inside_guard, offsets);
        }
        Expression::Access(Access::NullSafeProperty(pa)) => {
            collect_guarded_from_expr(pa.object, inside_guard, offsets);
        }
        Expression::ArrayAccess(aa) => {
            collect_guarded_from_expr(aa.array, inside_guard, offsets);
            collect_guarded_from_expr(aa.index, inside_guard, offsets);
        }
        Expression::Array(arr) => {
            for e in arr.elements.iter() {
                collect_guarded_from_array_elem(e, inside_guard, offsets);
            }
        }
        Expression::LegacyArray(arr) => {
            for e in arr.elements.iter() {
                collect_guarded_from_array_elem(e, inside_guard, offsets);
            }
        }
        Expression::Instantiation(inst) => {
            collect_guarded_from_expr(inst.class, inside_guard, offsets);
            if let Some(ref al) = inst.argument_list {
                for arg in al.arguments.iter() {
                    collect_guarded_from_expr(arg.value(), inside_guard, offsets);
                }
            }
        }
        // Don't recurse into closures/arrow functions.
        _ => {}
    }
}

fn collect_guarded_from_array_elem(
    elem: &ArrayElement<'_>,
    inside_guard: bool,
    offsets: &mut HashSet<u32>,
) {
    match elem {
        ArrayElement::KeyValue(kv) => {
            collect_guarded_from_expr(kv.key, inside_guard, offsets);
            collect_guarded_from_expr(kv.value, inside_guard, offsets);
        }
        ArrayElement::Value(v) => {
            collect_guarded_from_expr(v.value, inside_guard, offsets);
        }
        ArrayElement::Variadic(s) => {
            collect_guarded_from_expr(s.value, inside_guard, offsets);
        }
        ArrayElement::Missing(_) => {}
    }
}

/// Collect all variable offsets within an expression that is a target
/// of `isset()` or `empty()`.  This handles simple variables,
/// array access chains (`$arr['key']`), and property chains
/// (`$obj->prop`).
fn collect_guard_targets(expr: &Expression<'_>, offsets: &mut HashSet<u32>) {
    match expr {
        Expression::Variable(Variable::Direct(dv)) => {
            offsets.insert(dv.span().start.offset);
        }
        Expression::ArrayAccess(aa) => {
            collect_guard_targets(aa.array, offsets);
            // Don't mark the index expression as guarded.
        }
        Expression::Access(Access::Property(pa)) => {
            collect_guard_targets(pa.object, offsets);
        }
        Expression::Access(Access::NullSafeProperty(pa)) => {
            collect_guard_targets(pa.object, offsets);
        }
        Expression::Access(Access::StaticProperty(spa)) => {
            collect_guard_targets(spa.class, offsets);
        }
        _ => {}
    }
}
